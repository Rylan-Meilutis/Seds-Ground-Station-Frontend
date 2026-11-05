"""CI-only native platform build driver. Never deploys, signs or launches apps."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
PLATFORMS = json.loads((Path(__file__).parent / "platforms.json").read_text())
RUST = "1.99.0"
BINARYEN = {
    ("Linux", "x86_64"): ("x86_64-linux", "2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489"),
    ("Linux", "aarch64"): ("aarch64-linux", "89c07ea56faf38d0fbecf36ca8ec0721756716185f265b568e133d427f299bf8"),
    ("Darwin", "aarch64"): ("arm64-macos", "ad66da82ac13f163e424b1643f16c6dfcccc98b5966296b43e52d3cab04f84a8"),
    ("Darwin", "x86_64"): ("x86_64-macos", "13a9b90be775c6389ce3d1f879cb8627bea56708ba8c122983941d53a8199b95"),
    ("Windows", "x86_64"): ("x86_64-windows", "17a2cbeac6b5693c5fbafab3838d3c65fd9c1eb38b05f5baec6c657e8c84995b"),
}
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
WARNING = re.compile(r"(?i)(?:^|\s)(?:warning(?:\[[^]]+\])?:|warn(?:ing)?\s+(?!0\b)\S|w:\s)")


def file_sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def normalized_arch(value):
    return {"arm64": "aarch64", "amd64": "x86_64"}.get(value.lower(), value.lower())


def target_spec(name):
    return next(row for row in PLATFORMS if row["target"] == name)


def validate_host(name):
    spec = target_spec(name)
    actual = platform.system(), normalized_arch(platform.machine())
    if actual != (spec["host"], spec["arch"]):
        raise RuntimeError(f"{name} requires {spec['host']}/{spec['arch']}; actual {actual}")
    return actual


def build_command(name, profile):
    target_spec(name)
    if profile not in {"debug", "release"}:
        raise ValueError("profile must be debug or release")
    return [sys.executable, "build.py", "--target", name, "--" + profile, "--no-sign"]


def checked(argv, env, label):
    """Preserve full output and fail warnings as well as nonzero exits; no whitelist."""
    logs = ROOT / "ci-results"
    logs.mkdir(exist_ok=True)
    warning_lines = 0
    with (logs / (label + ".log")).open("w", encoding="utf-8") as log:
        child = subprocess.Popen(argv, cwd=ROOT, env=env, stdout=subprocess.PIPE,
                                 stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
        assert child.stdout is not None
        with child.stdout:
            for line in child.stdout:
                print(line, end="", flush=True)
                log.write(line)
                warning_lines += bool(WARNING.search(ANSI.sub("", line)))
        result = child.wait()
    (logs / (label + ".json")).write_text(json.dumps({"argv": argv, "exit": result, "warning_lines": warning_lines}, indent=2) + "\n")
    if result or warning_lines:
        raise RuntimeError(f"{label}: exit={result}, warning_lines={warning_lines}; inspect preserved log")


def ci_env():
    env = os.environ.copy()
    overrides = [name for name in env
                 if name in {"CARGO_ENCODED_RUSTFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS"}
                 or (name.startswith("CARGO_TARGET_") and name.endswith("_RUSTFLAGS"))]
    if overrides:
        raise RuntimeError(f"CI warning flags require a clean runner environment; remove {sorted(overrides)}")
    env.update({"RUSTUP_TOOLCHAIN": RUST, "CARGO_BUILD_JOBS": "2", "CARGO_INCREMENTAL": "0",
                "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0", "RUSTFLAGS": "-D warnings",
                "RUSTDOCFLAGS": "-D warnings", "CARGO_TERM_COLOR": "never", "NO_COLOR": "1"})
    # Never forward local deployment credentials into build-only jobs.
    for key in list(env):
        if key.startswith(("NOTARY_", "IOS_DISTRIBUTION_", "IOS_DEVELOPMENT_", "ANDROID_KEY")):
            env.pop(key)
    bins = [ROOT / ".ci-tools/binaryen/bin", ROOT / ".ci-tools/node/node_modules/.bin"]
    env["PATH"] = os.pathsep.join(map(str, bins)) + os.pathsep + env.get("PATH", "")
    return env


def install_binaryen(host):
    suffix, expected = BINARYEN[host]
    tools = ROOT / ".ci-tools"
    tools.mkdir(exist_ok=True)
    archive = tools / "binaryen.tar.gz"
    url = f"https://github.com/WebAssembly/binaryen/releases/download/version_133/binaryen-version_133-{suffix}.tar.gz"
    with urllib.request.urlopen(url, timeout=120) as response, archive.open("wb") as out:
        shutil.copyfileobj(response, out)
    if file_sha256(archive) != expected:
        raise RuntimeError("Binaryen archive checksum mismatch")
    dest = tools / "binaryen"
    dest.mkdir(exist_ok=False)
    with tarfile.open(archive) as bundle:
        # Pinned upstream archive, still reject traversal/special/link entries.
        for member in bundle.getmembers():
            pieces = Path(member.name).parts
            if not pieces or Path(member.name).is_absolute() or ".." in pieces:
                raise RuntimeError("Unsafe Binaryen archive path")
            if not (member.isdir() or member.isfile()):
                raise RuntimeError("Unsafe Binaryen archive member")
            relative = Path(*pieces[1:])
            if not relative.parts:
                continue
            path = dest / relative
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                with bundle.extractfile(member) as inp, path.open("wb") as out:
                    shutil.copyfileobj(inp, out)
                path.chmod(member.mode & 0o777)
    archive.unlink()


def resolved_tool_versions():
    """Match host CLI ABIs to the repository's actual resolved application crates."""
    lock = ROOT / "Cargo.lock"
    packages = tomllib.loads(lock.read_text(encoding="utf-8"))["package"]
    versions = {}
    for name in ["dioxus", "wasm-bindgen"]:
        found = {row["version"] for row in packages if row["name"] == name}
        if len(found) != 1 or not re.fullmatch(r"\d+\.\d+\.\d+", next(iter(found), "")):
            raise RuntimeError(f"Expected one resolved {name} version, found {sorted(found)}")
        versions[name] = found.pop()
    return versions


def bootstrap(name):
    host = validate_host(name)
    env = ci_env()
    for tool in ["rustup", "cargo", "npm", "node"]:
        if not shutil.which(tool):
            raise RuntimeError(f"Missing runner prerequisite: {tool}; see CI-RUNNERS.md")
    checked(["rustup", "toolchain", "install", RUST, "--profile", "minimal", "--component", "clippy"], env, "rust-toolchain")
    # Cargo.lock is ignored by this application repository. Resolve it in each
    # fresh CI checkout, then pin host CLI versions to the resulting crate ABIs.
    if not (ROOT / "Cargo.lock").exists():
        checked(["cargo", "generate-lockfile"], env, "resolve-application-lock")
    versions = resolved_tool_versions()
    receipt = ROOT / "ci-results/resolved-tools.json"
    receipt.parent.mkdir(exist_ok=True)
    receipt.write_text(json.dumps({"versions": versions,
                                  "lock_sha256": file_sha256(ROOT / "Cargo.lock")}, indent=2) + "\n")
    # Exact CLI versions; resolve their own dependencies afresh, matching the
    # app bootstrap policy for published CLI locks containing yanked crates.
    # Explicit selection avoids Cargo's implicit-toolchain warning during install.
    checked(["cargo", "+" + RUST, "install", "dioxus-cli", "--version", "=" + versions["dioxus"]], env, "dioxus-cli")
    checked(["cargo", "+" + RUST, "install", "wasm-bindgen-cli", "--version", "=" + versions["wasm-bindgen"]], env, "wasm-bindgen-cli")
    npm = shutil.which("npm")
    checked([npm, "install", "--prefix", str(ROOT / ".ci-tools/node"), "esbuild@0.25.12"], env, "esbuild")
    install_binaryen(host)
    checked(["wasm-opt", "--version"], env, "binaryen")
    # Needed by both the actual web target and common browser/tool bootstrap.
    checked(["rustup", "target", "add", "wasm32-unknown-unknown"], env, "wasm-target")


def run(name, profile):
    validate_host(name)
    env = ci_env()
    for tool in ["dx", "wasm-bindgen", "wasm-opt", "esbuild"]:
        if not shutil.which(tool, path=env["PATH"]):
            raise RuntimeError(f"Missing prepared tool: {tool}")
    if (ROOT / "artifacts" / name).exists():
        raise RuntimeError("Refusing stale platform artifact directory; use a fresh CI checkout")
    checked(build_command(name, profile), env, "build-" + name + "-" + profile)
    output = ROOT / "artifacts" / name
    files = sorted(p for p in output.rglob("*") if p.is_file())
    if not files:
        raise RuntimeError("Build produced no staged artifacts; not a successful platform build")
    manifest = {str(p.relative_to(ROOT)): {"bytes": p.stat().st_size,
                "sha256": file_sha256(p)} for p in files}
    (ROOT / "ci-results/artifacts.json").write_text(json.dumps(manifest, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["bootstrap", "build"])
    parser.add_argument("--target", choices=[p["target"] for p in PLATFORMS], default=os.environ.get("CI_FRONTEND_TARGET"), required=not bool(os.environ.get("CI_FRONTEND_TARGET")))
    parser.add_argument("--profile", choices=["debug", "release"], default=os.environ.get("CI_FRONTEND_PROFILE", "release"))
    args = parser.parse_args()
    if args.action == "bootstrap":
        bootstrap(args.target)
    else:
        run(args.target, args.profile)


if __name__ == "__main__":
    main()

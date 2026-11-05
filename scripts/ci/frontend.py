"""CI-only native platform build driver. Never deploys, signs or launches apps."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
import urllib.request
import urllib.error

ROOT = Path(__file__).resolve().parents[2]
PLATFORMS = json.loads((Path(__file__).parent / "platforms.json").read_text())
RUST = "1.99.0"
WASM_BINDGEN = {"0.2.130": {
    ("Linux", "x86_64"): ("x86_64-unknown-linux-musl", "9687f3bb551b9f66f37ed32a40d7913a00f7962db653054e56857bc306c3227f"),
    ("Linux", "aarch64"): ("aarch64-unknown-linux-gnu", "cc8dd8e90aa9111758168d30c63a14b7f0276997d9d60d3e160a1fb5e7d3b919"),
    ("Darwin", "aarch64"): ("aarch64-apple-darwin", "b90ddb9357f584b1af897d58ad77e4cce2b48fc17e2a967bc249a87f72809444"),
    ("Darwin", "x86_64"): ("x86_64-apple-darwin", "eb209c115b7cb95832423c1aa3f4d06d6c53f63ef74fc4b6fb79ef0b2eea5f10"),
    ("Windows", "x86_64"): ("x86_64-pc-windows-msvc", "844d23f1b56097549b936dd2e0d561fadebc1e8c9e985d5068b6b919654104ad"),
}}
BINARYEN = {
    ("Linux", "x86_64"): ("x86_64-linux", "2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489"),
    ("Linux", "aarch64"): ("aarch64-linux", "89c07ea56faf38d0fbecf36ca8ec0721756716185f265b568e133d427f299bf8"),
    ("Darwin", "aarch64"): ("arm64-macos", "ad66da82ac13f163e424b1643f16c6dfcccc98b5966296b43e52d3cab04f84a8"),
    ("Darwin", "x86_64"): ("x86_64-macos", "13a9b90be775c6389ce3d1f879cb8627bea56708ba8c122983941d53a8199b95"),
    ("Windows", "x86_64"): ("x86_64-windows", "17a2cbeac6b5693c5fbafab3838d3c65fd9c1eb38b05f5baec6c657e8c84995b"),
}
# Official linuxdeploy continuous release published 2026-11-01; pinned bytes,
# not trust in a mutable tag. A moved tag fails until explicitly reviewed.
LINUXDEPLOY = {
    "x86_64": ("linuxdeploy-x86_64.AppImage", "df908b19f1ed1cdafa79508d415aa12d6c56ce4d56d4149aa8c09286718706e5"),
    "aarch64": ("linuxdeploy-aarch64.AppImage", "f38e4d7cebf20d8073ec4e00e0a167ac146cd8585fe2f8fe82df27d74f2bd8e1"),
}
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
# Upstream Dioxus 0.7.10 workspace.rs emits these exact timer messages while
# successful metadata is pending. Preserve and report them separately from
# compiler/tool diagnostics; every other warning remains fatal.
DIOXUS_METADATA_PROGRESS = re.compile(r"^warning: (?:Waiting for cargo-metadata\.\.\.|\(Try (?:[1-9]|[1-9][0-9]|100)\) Taking a while\.\.\.)$")
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


def configure_utf8_console():
    """Keep UTF-8 child diagnostics printable on Windows redirected consoles."""
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if callable(reconfigure):
            reconfigure(encoding="utf-8", errors="strict")


def checked(argv, env, label, *, dioxus_progress=False):
    """Preserve full output and fail warnings as well as nonzero exits; no whitelist."""
    configure_utf8_console()
    logs = ROOT / "ci-results"
    logs.mkdir(exist_ok=True)
    warning_lines = 0
    progress_lines = []
    resolved = list(argv)
    if not Path(resolved[0]).is_absolute():
        executable = shutil.which(resolved[0], path=env.get("PATH", ""))
        if not executable:
            raise RuntimeError(f"Missing prepared executable: {resolved[0]}")
        resolved[0] = executable
    with (logs / (label + ".log")).open("w", encoding="utf-8") as log:
        child = subprocess.Popen(resolved, cwd=ROOT, env=env, stdout=subprocess.PIPE,
                                 stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
        assert child.stdout is not None
        with child.stdout:
            for line in child.stdout:
                print(line, end="", flush=True)
                log.write(line)
                plain = ANSI.sub("", line).strip()
                if dioxus_progress and DIOXUS_METADATA_PROGRESS.fullmatch(plain):
                    progress_lines.append(plain)
                else:
                    warning_lines += bool(WARNING.search(plain))
        result = child.wait()
    (logs / (label + ".json")).write_text(json.dumps({"argv": argv, "resolved_argv": resolved, "exit": result, "warning_lines": warning_lines, "external_metadata_progress_warnings": progress_lines}, indent=2) + "\n")
    if result or warning_lines:
        raise RuntimeError(f"{label}: exit={result}, warning_lines={warning_lines}; inspect preserved log")


def ci_env():
    env = os.environ.copy()
    env.update(PYTHONUTF8="1", PYTHONIOENCODING="utf-8")
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
    bins = [ROOT / ".ci-tools/wasm-bindgen", ROOT / ".ci-tools/binaryen/bin", ROOT / ".ci-tools/linuxdeploy/bin", ROOT / ".ci-tools/node/node_modules/.bin"]
    env["PATH"] = os.pathsep.join(map(str, bins)) + os.pathsep + env.get("PATH", "")
    return env


def wasm_bindgen_asset(host, version):
    try:
        return WASM_BINDGEN[version][host]
    except KeyError:
        raise RuntimeError(f"No pinned official wasm-bindgen {version} binary for {host}; update release checksums") from None


def download(url, archive, *, limit=64 * 1024 * 1024):
    """Bounded retries for transient transport failures; pins are checked by callers."""
    if archive.exists() or archive.is_symlink():
        raise RuntimeError("Refusing stale download path")
    for attempt in range(3):
        started = time.monotonic()
        try:
            with urllib.request.urlopen(url, timeout=120) as response, archive.open("xb") as out:
                total = 0
                while chunk := response.read(1024 * 1024):
                    total += len(chunk)
                    if total > limit or time.monotonic() - started > 120:
                        raise RuntimeError("Tool download exceeds its size bound or 120 seconds")
                    out.write(chunk)
            return
        except (urllib.error.URLError, TimeoutError, ConnectionError) as error:
            archive.unlink(missing_ok=True)
            transient = not isinstance(error, urllib.error.HTTPError) or error.code in {408, 429, 500, 502, 503, 504}
            if isinstance(error, urllib.error.HTTPError):
                error.close()
            if not transient or attempt == 2:
                raise
            time.sleep(1 << attempt)


def install_wasm_bindgen(host, version, env):
    suffix, expected = wasm_bindgen_asset(host, version)
    stem = f"wasm-bindgen-{version}-{suffix}"
    url = f"https://github.com/wasm-bindgen/wasm-bindgen/releases/download/{version}/{stem}.tar.gz"
    tools = ROOT / ".ci-tools"
    tools.mkdir(exist_ok=True)
    if tools.is_symlink():
        raise RuntimeError("Refusing linked CI tools directory")
    dest = tools / "wasm-bindgen"
    if dest.exists() or dest.is_symlink():
        raise RuntimeError("Refusing stale wasm-bindgen tool directory; use a fresh CI checkout")
    executable = "wasm-bindgen.exe" if host[0] == "Windows" else "wasm-bindgen"
    with tempfile.TemporaryDirectory(prefix="wasm-bindgen-", dir=tools) as temporary:
        stage = Path(temporary)
        archive = stage / "download.tar.gz"
        download(url, archive)
        actual = file_sha256(archive)
        if actual != expected:
            raise RuntimeError("wasm-bindgen archive checksum mismatch")
        selected = None
        with tarfile.open(archive) as bundle:
            total = 0
            for count, member in enumerate(bundle, 1):
                pieces = member.name.rstrip("/").split("/")
                if (count > 128 or not pieces or pieces[0] != stem
                        or any(p in {"", ".", ".."} for p in pieces)
                        or "\\" in member.name or ":" in member.name
                        or PurePosixPath(member.name).is_absolute()
                        or not (member.isdir() or member.isfile())):
                    raise RuntimeError("Unsafe wasm-bindgen archive member")
                total += member.size
                if member.size < 0 or total > 256 * 1024 * 1024:
                    raise RuntimeError("wasm-bindgen archive exceeds 256 MiB")
                if pieces == [stem, executable]:
                    if selected is not None or not member.isfile():
                        raise RuntimeError("Duplicate or invalid wasm-bindgen executable")
                    selected = member
            if selected is None:
                raise RuntimeError("Missing wasm-bindgen executable")
            # No archive path is used as an output path; only this verified file
            # is created in our private staging directory. Never extract links.
            binary = stage / executable
            with bundle.extractfile(selected) as source, binary.open("xb") as out:
                shutil.copyfileobj(source, out)
            binary.chmod(0o755)
        checked([str(binary), "--version"], env, "wasm-bindgen-version")
        reported = (ROOT / "ci-results/wasm-bindgen-version.log").read_text().strip()
        if reported != f"wasm-bindgen {version}":
            raise RuntimeError(f"wasm-bindgen version mismatch: {reported!r}")
        receipt = {"version": version, "host": host, "url": url,
                   "archive_sha256": actual, "binary_sha256": file_sha256(binary),
                   "archive_bytes": archive.stat().st_size, "reported": reported}
        archive.unlink()
        stage.rename(dest)
    (ROOT / "ci-results/wasm-bindgen-release.json").write_text(json.dumps(receipt, indent=2) + "\n")


def install_binaryen(host):
    suffix, expected = BINARYEN[host]
    tools = ROOT / ".ci-tools"
    tools.mkdir(exist_ok=True)
    archive = tools / "binaryen.tar.gz"
    url = f"https://github.com/WebAssembly/binaryen/releases/download/version_133/binaryen-version_133-{suffix}.tar.gz"
    # Official version133 Linux/Windows archives are 105-112 MB compressed.
    # Keep other tools' 64-MiB default; this bound still rejects oversized data.
    download(url, archive, limit=128 * 1024 * 1024)
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


def install_linuxdeploy(host):
    name, expected = LINUXDEPLOY[host[1]]
    tools = ROOT / ".ci-tools"
    destination = tools / "linuxdeploy"
    if destination.exists() or destination.is_symlink():
        raise RuntimeError("Refusing stale linuxdeploy directory")
    with tempfile.TemporaryDirectory(prefix="linuxdeploy-", dir=tools) as temporary:
        binary = Path(temporary) / "linuxdeploy"
        url = "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/" + name
        download(url, binary)
        if file_sha256(binary) != expected:
            raise RuntimeError("linuxdeploy checksum mismatch")
        binary.chmod(0o755)
        (destination / "bin").mkdir(parents=True)
        binary.rename(destination / "bin/linuxdeploy")
    (ROOT / "ci-results/linuxdeploy-release.json").write_text(json.dumps({
        "url": url, "host": host, "binary_sha256": expected,
        "upstream_release_id": 430855031,
    }, indent=2) + "\n")


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
    checked([sys.executable, "-m", "pip", "install", "Pillow==12.0.0"], env, "pillow")
    versions = resolved_tool_versions()
    # Resolve/download the exact application lock before dx's timer-based
    # metadata spinner; subsequent application Cargo work is offline/locked.
    checked(["cargo", "fetch", "--locked"], env, "application-fetch")
    wasm_bindgen_asset(host, versions["wasm-bindgen"])
    receipt = ROOT / "ci-results/resolved-tools.json"
    receipt.parent.mkdir(exist_ok=True)
    receipt.write_text(json.dumps({"versions": versions,
                                  "lock_sha256": file_sha256(ROOT / "Cargo.lock")}, indent=2) + "\n")
    # Exact CLI versions; resolve their own dependencies afresh, matching the
    # app bootstrap policy for published CLI locks containing yanked crates.
    # Explicit selection avoids Cargo's implicit-toolchain warning during install.
    checked(["cargo", "+" + RUST, "install", "dioxus-cli", "--version", "=" + versions["dioxus"]], env, "dioxus-cli")
    install_wasm_bindgen(host, versions["wasm-bindgen"], env)
    npm = shutil.which("npm")
    checked([npm, "install", "--prefix", str(ROOT / ".ci-tools/node"), "esbuild@0.25.12"], env, "esbuild")
    install_binaryen(host)
    if name in {"linux", "linux-arm64"}:
        for tool in ["makepkg", "fakeroot", "flatpak", "rpmbuild", "desktop-file-validate"]:
            if not shutil.which(tool, path=env["PATH"]):
                raise RuntimeError(f"Missing Linux packaging prerequisite: {tool}; see CI-RUNNERS.md")
        install_linuxdeploy(host)
    checked(["wasm-opt", "--version"], env, "binaryen")
    # Needed by both the actual web target and common browser/tool bootstrap.
    checked(["rustup", "target", "add", "wasm32-unknown-unknown"], env, "wasm-target")


def run(name, profile):
    validate_host(name)
    env = ci_env()
    env["CARGO_NET_OFFLINE"] = "true"
    for tool in ["dx", "wasm-bindgen", "wasm-opt", "esbuild"]:
        if not shutil.which(tool, path=env["PATH"]):
            raise RuntimeError(f"Missing prepared tool: {tool}")
    if (ROOT / "artifacts" / name).exists():
        raise RuntimeError("Refusing stale platform artifact directory; use a fresh CI checkout")
    checked(build_command(name, profile), env, "build-" + name + "-" + profile, dioxus_progress=True)
    output = ROOT / "artifacts" / name
    files = sorted(p for p in output.rglob("*") if p.is_file())
    if not files:
        raise RuntimeError("Build produced no staged artifacts; not a successful platform build")
    manifest = {str(p.relative_to(ROOT)): {"bytes": p.stat().st_size,
                "sha256": file_sha256(p)} for p in files}
    (ROOT / "ci-results/artifacts.json").write_text(json.dumps(manifest, indent=2) + "\n")


def main():
    configure_utf8_console()
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

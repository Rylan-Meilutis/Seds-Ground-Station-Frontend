"""Frontend CI driver.

`bootstrap`/`build` are build-only CI: Android uses ephemeral development
signing and never production credentials. The `release-*` actions run only in
the tag-triggered release workflow (.github/workflows/release.yml), which is
the one place production signing material (GitHub secrets) is used.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import secrets
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
# Official latest non-prerelease at source review (2026-11-06), release
# 270526371 / asset 329035725. Pin bytes, never a moving latest download.
BUNDLETOOL = {
    "version": "1.18.3",
    "url": "https://github.com/google/bundletool/releases/download/1.18.3/bundletool-all-1.18.3.jar",
    "sha256": "a099cfa1543f55593bc2ed16a70a7c67fe54b1747bb7301f37fdfd6d91028e29",
    "bytes": 32520401,
}
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
# Upstream Dioxus 0.7.10 workspace.rs emits these exact timer messages while
# successful metadata is pending. Preserve and report them separately from
# compiler/tool diagnostics; every other warning remains fatal.
DIOXUS_METADATA_PROGRESS = re.compile(r"^warning: (?:Waiting for cargo-metadata\.\.\.|\(Try (?:[1-9]|[1-9][0-9]|100)\) Taking a while\.\.\.)$")
# AGP's default keep rules and AndroidX consumer rules trigger this exact R8
# forward-compatibility notice during every minified release. Those files are
# generated from Gradle's downloads; the notice needs no action until R8's next
# major version. Rules owned by this repository or any other R8 text stay fatal.
ANDROID_R8_UPSTREAM_RULE_NOTICE = re.compile(
    r"^WARNING: \S*(?:/build/intermediates/default_proguard_files/global/proguard-android(?:-optimize)?\.txt-[0-9.]+"
    r"|/\.gradle-user-home/caches/[0-9.]+/transforms/[0-9a-f]+/transformed/[^/\s]+(?:/proguard\.txt|/lib/META-INF/proguard/[^/\s]+\.pro))"
    r":[0-9]+:[0-9]+-[0-9]+:[0-9]+: R8: The current version of R8 implicitly keeps the default constructor for Proguard configuration rules "
    r"that have no member pattern\. If the following rule should continue to keep the default constructor in the next major version of R8, "
    r"then it must be augmented with the member pattern `\{ void <init>\(\); \}` to explicitly keep the default constructor:$"
)
WARNING = re.compile(r"(?i)(?:(?:^|\s)(?:warning(?:\[[^]]+\])?:|warn(?:ing)?\s+(?!0\b)\S|w:\s)|::warning(?: [^:]*)?::)")

# Only the two observed Cargo/libcurl error-55 SSL transport diagnostics may
# trigger a clean offline verification after an otherwise successful fetch.
CARGO_FETCH_TRANSIENT = re.compile(
    r"^warning: spurious network error \([1-3] tries remaining\): \[55\] "
    r"Failed sending data to the peer \(OpenSSL SSL_read: "
    r"(?:SSL_ERROR_SYSCALL|OpenSSL/[0-9.]+: error:0A000126:SSL routines::unexpected eof while reading), errno 0\)$"
)


class CommandFailure(RuntimeError):
    def __init__(self, label, result, warnings):
        super().__init__(f"{label}: exit={result}, warning_lines={len(warnings)}; inspect preserved log")
        self.result = result
        self.warnings = tuple(warnings)


def file_sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def normalized_arch(value):
    return {"arm64": "aarch64", "amd64": "x86_64"}.get(value.lower(), value.lower())


def target_spec(name):
    return next(row for row in PLATFORMS if row["target"] == name)


def is_wear(name):
    """Wear OS companion targets: a Kotlin/Gradle build with no Rust."""
    return name.startswith("wearos")


def android_signed(name):
    return name.startswith("android") or is_wear(name)


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


def checked(argv, env, label, *, dioxus_progress=False, android_upstream_r8=False):
    """Preserve full output and fail warnings as well as nonzero exits.

    Only the exact upstream notices named above are reported separately, and
    only for the build step which opts in.
    """
    configure_utf8_console()
    logs = ROOT / "ci-results"
    logs.mkdir(exist_ok=True)
    warnings = []
    progress_lines = []
    upstream_r8_lines = []
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
                elif android_upstream_r8 and ANDROID_R8_UPSTREAM_RULE_NOTICE.fullmatch(plain):
                    upstream_r8_lines.append(plain)
                else:
                    if WARNING.search(plain):
                        warnings.append(plain)
        result = child.wait()
    (logs / (label + ".json")).write_text(json.dumps({"argv": argv, "resolved_argv": resolved, "exit": result, "warning_lines": len(warnings), "external_metadata_progress_warnings": progress_lines, "external_android_r8_rule_notices": upstream_r8_lines}, indent=2) + "\n")
    if result or warnings:
        raise CommandFailure(label, result, warnings)


def fetch_application(env):
    """Require a clean fetch, or verify an SSL-recovered fetch fully offline."""
    try:
        checked(["cargo", "fetch", "--locked"], env, "application-fetch")
    except CommandFailure as failure:
        if (failure.result != 0 or not failure.warnings
                or not all(CARGO_FETCH_TRANSIENT.fullmatch(line) for line in failure.warnings)):
            raise
        # Keep the initial log/receipt unchanged. No second network attempt and
        # no warning exemption in the verification or subsequent build steps.
        checked(["cargo", "fetch", "--locked", "--offline"], env,
                "application-fetch-offline-verification")


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
        if key.startswith(("NOTARY_", "IOS_DISTRIBUTION_", "IOS_DEVELOPMENT_", "ANDROID_KEY", "ANDROID_UPLOAD_",
                           "SEDS_GS_CI_ANDROID_", "SEDS_GS_RELEASE_", "SEDS_GS_NOTARY_", "MACOS_SIGNING_",
                           "MACOS_DEVELOPER_ID_", "APPLE_NOTARY_", "APPLE_APP_SPECIFIC_")) or key in {"APPLE_ID", "APPLE_TEAM_ID"}:
            env.pop(key)
    bins = [ROOT / ".ci-tools/wasm-bindgen", ROOT / ".ci-tools/binaryen/bin", ROOT / ".ci-tools/linuxdeploy/bin", ROOT / ".ci-tools/node/node_modules/.bin"]
    env["PATH"] = os.pathsep.join(map(str, bins)) + os.pathsep + env.get("PATH", "")
    # build.py's repo-managed AAB -> universal APK stage consumes this value;
    # dx itself builds the AAB through Gradle and need not read this variable.
    env["BUNDLETOOL_JAR"] = str(ROOT / ".ci-tools/bundletool/bundletool.jar")
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


def install_bundletool(env):
    tools = ROOT / ".ci-tools"
    tools.mkdir(exist_ok=True)
    if tools.is_symlink():
        raise RuntimeError("Refusing linked CI tools directory")
    destination = tools / "bundletool"
    if destination.exists() or destination.is_symlink():
        raise RuntimeError("Refusing stale bundletool directory; use a fresh CI checkout")
    if not shutil.which("java", path=env["PATH"]):
        raise RuntimeError("Missing Android prerequisite: Java 17; see CI-RUNNERS.md")
    with tempfile.TemporaryDirectory(prefix="bundletool-", dir=tools) as temporary:
        stage = Path(temporary)
        jar = stage / "bundletool.jar"
        download(BUNDLETOOL["url"], jar)
        if jar.stat().st_size != BUNDLETOOL["bytes"] or file_sha256(jar) != BUNDLETOOL["sha256"]:
            raise RuntimeError("bundletool release size/checksum mismatch")
        checked(["java", "-jar", str(jar), "version"], env, "bundletool-version")
        reported = (ROOT / "ci-results/bundletool-version.log").read_text().strip()
        if reported != BUNDLETOOL["version"]:
            raise RuntimeError(f"bundletool version mismatch: {reported!r}")
        stage.rename(destination)
    (ROOT / "ci-results/bundletool-release.json").write_text(json.dumps({
        **BUNDLETOOL, "reported": reported, "upstream_release_id": 270526371,
        "upstream_asset_id": 329035725,
    }, indent=2) + "\n")


def verify_bundletool(env):
    jar = ROOT / ".ci-tools/bundletool/bundletool.jar"
    if (env.get("BUNDLETOOL_JAR") != str(jar) or not jar.is_file()
            or jar.is_symlink() or jar.parent.is_symlink() or jar.parent.parent.is_symlink()
            or jar.stat().st_size != BUNDLETOOL["bytes"] or file_sha256(jar) != BUNDLETOOL["sha256"]):
        raise RuntimeError("Missing or changed prepared bundletool release")


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
    if is_wear(name):
        # Gradle wrapper (pinned in watch/wear/gradle/wrapper) plus the runner's
        # JDK and Android SDK; no Rust or web tooling is involved.
        if not shutil.which("java", path=env["PATH"]):
            raise RuntimeError("Missing Wear OS prerequisite: Java 17 or 21; see CI-RUNNERS.md")
        if not (env.get("ANDROID_HOME") or env.get("ANDROID_SDK_ROOT")):
            raise RuntimeError("Missing Wear OS prerequisite: Android SDK (ANDROID_HOME); see CI-RUNNERS.md")
        (ROOT / "ci-results").mkdir(exist_ok=True)
        (ROOT / ".ci-tools").mkdir(exist_ok=True)
        return
    if name.startswith("watchos") and not shutil.which("xcodegen", path=env["PATH"]):
        raise RuntimeError("Missing watchOS prerequisite: xcodegen (brew install xcodegen); see CI-RUNNERS.md")
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
    fetch_application(env)
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
    if name.startswith("android"):
        install_bundletool(env)
    if name in {"linux", "linux-arm64"}:
        for tool in ["makepkg", "bsdtar", "pacman", "fakeroot", "flatpak", "rpmbuild", "desktop-file-validate", "appstreamcli", "gpg"]:
            if not shutil.which(tool, path=env["PATH"]):
                raise RuntimeError(f"Missing Linux packaging prerequisite: {tool}; see CI-RUNNERS.md")
        install_linuxdeploy(host)
    checked(["wasm-opt", "--version"], env, "binaryen")
    # Needed by both the actual web target and common browser/tool bootstrap.
    checked(["rustup", "target", "add", "wasm32-unknown-unknown"], env, "wasm-target")


def prepare_android_development_signing(env):
    """Create a fresh job-local key. It is not a distribution identity or an update key."""
    directory = ROOT / ".ci-tools/android-development-signing"
    directory.mkdir(mode=0o700)
    key = directory / "development.p12"
    env["SEDS_GS_CI_ANDROID_KEYSTORE"] = str(key)
    password_file = directory / "password.txt"
    descriptor = os.open(password_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w") as stream:
        stream.write(secrets.token_urlsafe(24) + "\n")
    env["SEDS_GS_CI_ANDROID_KEYSTORE_PASSWORD_FILE"] = str(password_file)
    checked(["keytool", "-genkeypair", "-noprompt", "-storetype", "PKCS12",
             "-keystore", str(key), "-alias", "androiddebugkey", "-keyalg", "RSA",
             "-keysize", "2048", "-validity", "3650",
             "-dname", "CN=Android Debug,O=Android,C=US",
             "-storepass:file", str(password_file),
             "-keypass:file", str(password_file)],
            env, "android-development-key")
    if not key.is_file() or key.is_symlink():
        raise RuntimeError("Development key generation produced no regular keystore")
    key.chmod(0o600)
    (ROOT / "ci-results/android-development-signing.json").write_text(json.dumps({
        "mode": "ephemeral-ci-development-only", "production_credentials_used": False,
        "alias": "androiddebugkey", "keystore_uploaded": False, "password_file_uploaded": False,
        "distribution_or_update_identity": False}, indent=2) + "\n")


def run(name, profile):
    validate_host(name)
    env = ci_env()
    env["CARGO_NET_OFFLINE"] = "true"
    if name.startswith("android"):
        verify_bundletool(env)
    for tool in [] if is_wear(name) else ["dx", "wasm-bindgen", "wasm-opt", "esbuild"]:
        if not shutil.which(tool, path=env["PATH"]):
            raise RuntimeError(f"Missing prepared tool: {tool}")
    if (ROOT / "artifacts" / name).exists():
        raise RuntimeError("Refusing stale platform artifact directory; use a fresh CI checkout")
    if android_signed(name):
        prepare_android_development_signing(env)
    checked(build_command(name, profile), env, "build-" + name + "-" + profile, dioxus_progress=True,
            android_upstream_r8=name.startswith("android") and profile == "release")
    output = ROOT / "artifacts" / name
    files = sorted(p for p in output.rglob("*") if p.is_file())
    if not files:
        raise RuntimeError("Build produced no staged artifacts; not a successful platform build")
    if android_signed(name):
        (output / "CI-DEVELOPMENT-SIGNED-NOT-FOR-DISTRIBUTION.txt").write_text(
            "APK signed with a fresh CI-only development certificate.\n"
            "Not a production release or an identity for updating installed releases.\n"
            "The private keystore is not included in this artifact.\n")
        files = sorted(p for p in output.rglob("*") if p.is_file())
    manifest = {str(p.relative_to(ROOT)): {"bytes": p.stat().st_size,
                "sha256": file_sha256(p)} for p in files}
    (ROOT / "ci-results/artifacts.json").write_text(json.dumps(manifest, indent=2) + "\n")


# ---------------------------------------------------------------------------
# Tag-triggered releases (.github/workflows/release.yml only).
#
# Same bootstrap and build.py code paths as build-only CI, but `--final` for
# Android/macOS: build.py signs with the production identity exactly as it does
# locally (_configure_android_signing / _finalize_android_release, and
# sign_macos_app_and_dmg / notarize_macos). The signing material comes from
# GitHub secrets, is written only under $RUNNER_TEMP, and is deleted by
# `release-cleanup` in an always() step. Nothing here falls back to a
# development key or an unsigned artifact.

RELEASE_TARGETS = ("linux", "linux-arm64", "windows", "android", "android-armv7", "macos", "macos-intel")
SIGNED_RELEASE_TARGETS = {"android": "android", "android-armv7": "android", "macos": "macos", "macos-intel": "macos"}
RELEASE_TAG = re.compile(r"^v(\d+\.\d+\.\d+)b(\d+)$")
# Published names match the earlier GitHub releases (GitHub turns spaces in
# uploaded file names into dots), so existing links keep the same shape.
RELEASE_ASSETS = {
    "windows": {"UBSEDS GroundStation Installer.exe": "UBSEDS.GroundStation.Installer.exe"},
    "linux": {
        "UBSEDS GS_x86_64.AppImage": "UBSEDS.GS_x86_64.AppImage",
        "UBSEDS GS_x86_64.flatpak": "UBSEDS.GS_x86_64.flatpak",
        "UBSEDS GS_x86_64.pkg.tar.gz": "UBSEDS.GS_x86_64.pkg.tar.gz",
        "UBSEDS GS_amd64.deb": "UBSEDS.GS_amd64.deb",
        "UBSEDS GS_x86_64.rpm": "UBSEDS.GS_x86_64.rpm",
    },
    "linux-arm64": {
        "UBSEDS GS_aarch64.AppImage": "UBSEDS.GS_aarch64.AppImage",
        "UBSEDS GS_aarch64.flatpak": "UBSEDS.GS_aarch64.flatpak",
        "UBSEDS GS_aarch64.pkg.tar.gz": "UBSEDS.GS_aarch64.pkg.tar.gz",
        "UBSEDS GS_arm64.deb": "UBSEDS.GS_arm64.deb",
        "UBSEDS GS_aarch64.rpm": "UBSEDS.GS_aarch64.rpm",
    },
    "android": {"SEDS GS.apk": "SEDS.GS.apk"},
    "android-armv7": {"SEDS GS.apk": "SEDS.GS_armv7.apk"},
    "macos": {"UBSEDS GS.dmg": "UBSEDS.GS.dmg"},
    "macos-intel": {"UBSEDS GS.dmg": "UBSEDS.GS_x86_64.dmg"},
}
SIGNING_MATERIAL_SUFFIXES = (".jks", ".keystore", ".p12", ".pfx", ".p8", ".pem", ".key",
                             ".keychain", ".keychain-db", ".mobileprovision", ".provisionprofile")
# Signing variables release_env() passes into build.py, per signed platform.
RELEASE_SIGNING_ENV = {
    "android": ("ANDROID_KEYSTORE_PATH", "ANDROID_KEYSTORE_PASSWORD", "ANDROID_KEY_PASSWORD",
                "ANDROID_KEY_ALIAS", "ANDROID_KEYSTORE_TYPE"),
    "macos": ("MACOS_SIGNING_IDENTITY", "NOTARY_PROFILE", "NOTARY_KEYCHAIN"),
}
ANDROID_RELEASE_SECRETS = ("ANDROID_UPLOAD_KEYSTORE_B64", "ANDROID_KEYSTORE_PASSWORD")
MACOS_CERT_SECRETS = ("MACOS_DEVELOPER_ID_P12_B64", "MACOS_DEVELOPER_ID_P12_PASSWORD")
NOTARY_API_SECRETS = ("APPLE_NOTARY_KEY_P8_B64", "APPLE_NOTARY_KEY_ID", "APPLE_NOTARY_ISSUER_ID")
NOTARY_APPLE_ID_SECRETS = ("APPLE_ID", "APPLE_APP_SPECIFIC_PASSWORD", "APPLE_TEAM_ID")
NOTARY_PROFILE_NAME = "seds-gs-release"
KEYCHAIN_NAME = "release.keychain-db"


def release_version(tag):
    """The tag must name exactly Cargo.toml's version and Dioxus.toml's build."""
    match = RELEASE_TAG.fullmatch(tag or "")
    if not match:
        raise RuntimeError(f"Release tag must look like v<major>.<minor>.<patch>b<build> (e.g. v0.3.1b36), got {tag!r}")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    build = str(tomllib.loads((ROOT / "Dioxus.toml").read_text(encoding="utf-8"))["application"]["build"])
    if (match.group(1), match.group(2)) != (version, build):
        raise RuntimeError(f"Tag {tag} does not match Cargo.toml version {version} and Dioxus.toml build {build}; "
                           f"expected v{version}b{build}")
    major, minor, patch = map(int, version.split("."))
    return {"tag": tag, "version": version, "build": build, "name": f"Version {version} Build {build}",
            "version_code": major * 10_000_000 + minor * 100_000 + patch * 1_000 + int(build)}


def release_command(name):
    if name not in RELEASE_TARGETS:
        raise ValueError(f"{name} is not a release target")
    signing = ["--final"] if name in SIGNED_RELEASE_TARGETS else ["--no-sign"]
    return [sys.executable, "build.py", "--target", name, "--release", *signing]


def release_signing_dir(create=False):
    """Job-local signing directory under $RUNNER_TEMP, never inside the checkout."""
    temp = os.environ.get("RUNNER_TEMP", "").strip()
    if not temp:
        raise RuntimeError("Release signing needs RUNNER_TEMP (the GitHub Actions job temp directory)")
    directory = Path(temp).resolve() / "seds-release-signing"
    if directory == ROOT or ROOT in directory.parents:
        raise RuntimeError("Release signing directory must be outside the checkout")
    if create:
        directory.mkdir(mode=0o700)
        directory.chmod(0o700)
    return directory


def private_write(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(data)


def decode_secret(name):
    raw = "".join(os.environ.get(name, "").split())
    try:
        data = base64.b64decode(raw, validate=True)
    except ValueError:
        raise RuntimeError(f"Secret {name} is not valid base64") from None
    if not data:
        raise RuntimeError(f"Secret {name} is empty")
    return data


def require_secrets(names, what):
    missing = [name for name in names if not os.environ.get(name, "").strip()]
    if missing:
        raise RuntimeError(f"{what}: missing GitHub secret(s) {', '.join(missing)}. Add them as described in "
                           "docs/releasing.md. A release is never signed with a development key or shipped unsigned.")


def github_file(variable, values):
    """Append name=value lines to $GITHUB_ENV / $GITHUB_OUTPUT (when set)."""
    path = os.environ.get(variable, "").strip()
    for key, value in values.items():
        if "\n" in str(value) or "\r" in str(value):
            raise RuntimeError(f"Refusing multi-line {variable} value for {key}")
    if path:
        with open(path, "a", encoding="utf-8") as stream:
            for key, value in values.items():
                stream.write(f"{key}={value}\n")


def job_summary(lines):
    text = "\n".join(lines) + "\n"
    print(text, end="")
    path = os.environ.get("GITHUB_STEP_SUMMARY", "").strip()
    if path:
        with open(path, "a", encoding="utf-8") as stream:
            stream.write(text)


def quiet_run(argv, label, *, env=None, capture=False):
    """Run a signing command without echoing or recording its argv (it may
    carry a secret). Output is printed only on failure, or returned."""
    result = subprocess.run(argv, env=env, capture_output=True)
    if result.returncode:
        detail = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        raise RuntimeError(f"{label} failed (exit {result.returncode}):\n{detail}")
    return result.stdout if capture else None


def prepare_android_release_signing():
    require_secrets(ANDROID_RELEASE_SECRETS, "Production Android signing")
    alias = os.environ.get("ANDROID_KEY_ALIAS", "").strip() or "upload"
    directory = release_signing_dir(create=True)
    keystore = directory / "upload.jks"
    private_write(keystore, decode_secret("ANDROID_UPLOAD_KEYSTORE_B64"))
    # Passwords reach keytool through its environment, never its argv.
    env = dict(os.environ, SEDS_GS_STOREPASS=os.environ["ANDROID_KEYSTORE_PASSWORD"].strip())
    der = quiet_run(["keytool", "-exportcert", "-keystore", str(keystore), "-alias", alias,
                     "-storepass:env", "SEDS_GS_STOREPASS"], f"Reading alias {alias!r} from the production keystore",
                    env=env, capture=True)
    if not der:
        raise RuntimeError(f"Production keystore has no certificate for alias {alias!r}")
    fingerprint = hashlib.sha256(der).hexdigest()
    github_file("GITHUB_ENV", {"ANDROID_KEYSTORE_PATH": str(keystore),
                               "SEDS_GS_RELEASE_ANDROID_CERT_SHA256": fingerprint})
    job_summary([f"Android production keystore loaded (alias `{alias}`), certificate SHA-256 `{colon_hex(fingerprint)}`."])


def colon_hex(digest):
    digest = digest.upper()
    return ":".join(digest[i:i + 2] for i in range(0, len(digest), 2))


def normalized_digest(value):
    return re.sub(r"[^0-9a-f]", "", (value or "").lower())


def security_keychains():
    out = quiet_run(["security", "list-keychains", "-d", "user"], "Listing keychains", capture=True)
    return [line.strip().strip('"') for line in out.decode("utf-8", errors="replace").splitlines() if line.strip()]


def codesigning_identities(keychain):
    out = quiet_run(["security", "find-identity", "-v", "-p", "codesigning", str(keychain)],
                    "Listing code signing identities", capture=True).decode("utf-8", errors="replace")
    return re.findall(r'^\s*\d+\)\s+[0-9A-F]{40}\s+"([^"]+)"\s*$', out, re.MULTILINE)


def notary_mode():
    api = [name for name in NOTARY_API_SECRETS if os.environ.get(name, "").strip()]
    apple_id = [name for name in NOTARY_APPLE_ID_SECRETS if os.environ.get(name, "").strip()]
    if len(api) == len(NOTARY_API_SECRETS):
        return "api-key"
    if api:
        require_secrets(NOTARY_API_SECRETS, "Notarization with an App Store Connect API key")
    if len(apple_id) == len(NOTARY_APPLE_ID_SECRETS):
        return "apple-id"
    if apple_id:
        require_secrets(NOTARY_APPLE_ID_SECRETS, "Notarization with an Apple ID")
    raise RuntimeError("Apple notarization: missing GitHub secrets. Set APPLE_NOTARY_KEY_P8_B64, "
                       "APPLE_NOTARY_KEY_ID and APPLE_NOTARY_ISSUER_ID (preferred), or APPLE_ID, "
                       "APPLE_APP_SPECIFIC_PASSWORD and APPLE_TEAM_ID; see docs/releasing.md. "
                       "A DMG is never released without notarization.")


def prepare_macos_release_signing():
    require_secrets(MACOS_CERT_SECRETS, "Developer ID signing")
    mode = notary_mode()
    directory = release_signing_dir(create=True)
    keychain = directory / KEYCHAIN_NAME
    password = secrets.token_urlsafe(32)
    print(f"::add-mask::{password}", flush=True)
    quiet_run(["security", "create-keychain", "-p", password, str(keychain)], "Creating the temporary keychain")
    quiet_run(["security", "set-keychain-settings", "-lut", "21600", str(keychain)], "Setting the keychain timeout")
    quiet_run(["security", "unlock-keychain", "-p", password, str(keychain)], "Unlocking the temporary keychain")
    p12 = directory / "developer-id.p12"
    private_write(p12, decode_secret("MACOS_DEVELOPER_ID_P12_B64"))
    try:
        quiet_run(["security", "import", str(p12), "-k", str(keychain), "-f", "pkcs12",
                   "-P", os.environ["MACOS_DEVELOPER_ID_P12_PASSWORD"],
                   "-T", "/usr/bin/codesign", "-T", "/usr/bin/security"], "Importing the Developer ID certificate")
    finally:
        p12.unlink(missing_ok=True)
    quiet_run(["security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s",
               "-k", password, str(keychain)], "Allowing codesign to use the key")
    others = [path for path in security_keychains() if Path(path) != keychain]
    quiet_run(["security", "list-keychains", "-d", "user", "-s", str(keychain), *others],
              "Adding the temporary keychain to the search list")
    identities = codesigning_identities(keychain)
    wanted = os.environ.get("MACOS_SIGNING_IDENTITY", "").strip()
    if wanted:
        if wanted not in identities:
            raise RuntimeError("MACOS_SIGNING_IDENTITY is not a valid identity in the imported certificate; found: "
                               + (", ".join(identities) or "none"))
        identity = wanted
    else:
        developer_id = [name for name in identities if name.startswith("Developer ID Application:")]
        if len(developer_id) != 1:
            raise RuntimeError("Expected exactly one Developer ID Application identity in the imported .p12; found: "
                               + (", ".join(identities) or "none") + ". Set MACOS_SIGNING_IDENTITY.")
        identity = developer_id[0]
    # Both notarization modes end as a notarytool profile inside the temporary
    # keychain, so build.py's notarize_macos runs unchanged (NOTARY_PROFILE +
    # NOTARY_KEYCHAIN) and no credential is on a command line during the build.
    store = ["xcrun", "notarytool", "store-credentials", NOTARY_PROFILE_NAME, "--keychain", str(keychain)]
    if mode == "api-key":
        key = directory / "notary-api-key.p8"
        private_write(key, decode_secret("APPLE_NOTARY_KEY_P8_B64"))
        try:
            quiet_run(store + ["--key", str(key), "--key-id", os.environ["APPLE_NOTARY_KEY_ID"].strip(),
                               "--issuer", os.environ["APPLE_NOTARY_ISSUER_ID"].strip()],
                      "Storing the notarization API key profile")
        finally:
            key.unlink(missing_ok=True)
    else:
        quiet_run(store + ["--apple-id", os.environ["APPLE_ID"].strip(), "--team-id", os.environ["APPLE_TEAM_ID"].strip(),
                           "--password", os.environ["APPLE_APP_SPECIFIC_PASSWORD"].strip()],
                  "Storing the notarization Apple ID profile")
    github_file("GITHUB_ENV", {"MACOS_SIGNING_IDENTITY": identity, "NOTARY_PROFILE": NOTARY_PROFILE_NAME,
                               "NOTARY_KEYCHAIN": str(keychain)})
    job_summary([f"macOS signing identity: `{identity}`",
                 f"Notarization credentials: {'App Store Connect API key' if mode == 'api-key' else 'Apple ID (app-specific password)'}"])


def release_env(name):
    """ci_env() (which drops every signing variable), plus exactly the signing
    variables build.py needs for this target's platform, and nothing else."""
    kind = SIGNED_RELEASE_TARGETS.get(name)
    env = ci_env()
    for key in RELEASE_SIGNING_ENV.get(kind, ()):
        value = os.environ.get(key, "").strip()
        if value:
            env[key] = value
    if kind == "android":
        missing = [key for key in ("ANDROID_KEYSTORE_PATH", "ANDROID_KEYSTORE_PASSWORD") if not env.get(key)]
        if missing:
            raise RuntimeError("Production Android signing is not prepared (missing " + ", ".join(missing)
                               + "); run release-android-signing with the GitHub secrets first")
        if Path(env["ANDROID_KEYSTORE_PATH"]).resolve().parent != release_signing_dir():
            raise RuntimeError("The release keystore must be the job-local file written by release-android-signing")
    elif kind == "macos":
        missing = [key for key in RELEASE_SIGNING_ENV["macos"] if not env.get(key)]
        if missing:
            raise RuntimeError("Developer ID signing/notarization is not prepared (missing " + ", ".join(missing)
                               + "); run release-macos-signing with the GitHub secrets first")
        env["SEDS_GS_NOTARY_RECEIPT"] = str(ROOT / "ci-results/notarization.json")
    return env


def release_build(name):
    validate_host(name)
    env = release_env(name)
    env["CARGO_NET_OFFLINE"] = "true"
    if name.startswith("android"):
        verify_bundletool(env)
    for tool in ["dx", "wasm-bindgen", "wasm-opt", "esbuild"]:
        if not shutil.which(tool, path=env["PATH"]):
            raise RuntimeError(f"Missing prepared tool: {tool}")
    if (ROOT / "artifacts" / name).exists():
        raise RuntimeError("Refusing stale platform artifact directory; use a fresh checkout")
    (ROOT / "ci-results").mkdir(exist_ok=True)
    checked(release_command(name), env, "release-" + name, dioxus_progress=True,
            android_upstream_r8=name.startswith("android"))


def find_apksigner():
    sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT") or ""
    def version(path):
        return tuple(int(p) if p.isdigit() else 0 for p in re.split(r"[.-]", path.parent.name))
    candidates = sorted(Path(sdk).glob("build-tools/*/apksigner"), key=version) if sdk else []
    found = str(candidates[-1]) if candidates else shutil.which("apksigner")
    if not found:
        raise RuntimeError("apksigner not found in the Android SDK build-tools")
    return found


def apk_signers(output):
    """(DN, SHA-256) for each signer in `apksigner verify --print-certs` output."""
    count = re.search(r"^Number of signers: (\d+)$", output, re.MULTILINE)
    digests = re.findall(r"^Signer #\d+ certificate SHA-256 digest: ([0-9a-f]{64})$", output, re.MULTILINE)
    names = re.findall(r"^Signer #\d+ certificate DN: (.+)$", output, re.MULTILINE)
    if not count or int(count.group(1)) != len(digests) or len(names) != len(digests):
        raise RuntimeError("Could not read the APK signer certificates from apksigner output")
    return list(zip(names, digests))


def verify_android_release(name):
    apk = ROOT / "artifacts" / name / "dist" / "SEDS GS.apk"
    if not apk.is_file():
        raise RuntimeError(f"Release APK missing: {apk}")
    result = subprocess.run([find_apksigner(), "verify", "--print-certs", "--verbose", str(apk)],
                            capture_output=True, text=True)
    print(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError(f"apksigner verify failed (exit {result.returncode})")
    if not re.search(r"^Verified using v2 scheme \(APK Signature Scheme v2\): true$", result.stdout, re.MULTILINE):
        raise RuntimeError("Release APK is not signed with APK Signature Scheme v2")
    signers = apk_signers(result.stdout)
    expected = normalized_digest(os.environ.get("SEDS_GS_RELEASE_ANDROID_CERT_SHA256"))
    if len(signers) != 1 or not expected or signers[0][1] != expected:
        raise RuntimeError("Release APK is not signed by exactly the production keystore certificate")
    pinned = normalized_digest(os.environ.get("ANDROID_EXPECTED_CERT_SHA256"))
    if pinned and pinned != expected:
        raise RuntimeError("Production keystore certificate does not match the ANDROID_EXPECTED_CERT_SHA256 "
                           "repository variable; refusing to publish an APK that cannot update installed releases")
    dn, digest = signers[0]
    job_summary([f"### {name} APK signature",
                 f"- Signer: `{dn}`",
                 f"- Certificate SHA-256: `{colon_hex(digest)}`",
                 f"- Matches pinned ANDROID_EXPECTED_CERT_SHA256: {'yes' if pinned else 'no pin set'}"])


def verify_macos_release(name):
    # The original dist/ bundle: build_targets copies dist/ into artifacts/
    # without preserving symlinks, so that copy of the .app is not verifiable.
    dist = ROOT / "dist"
    app, dmg = dist / "UBSEDS GS.app", dist / "UBSEDS GS.dmg"
    staged = ROOT / "artifacts" / name / "dist" / "UBSEDS GS.dmg"
    for path in (app, dmg, staged):
        if not path.exists():
            raise RuntimeError(f"Release macOS output missing: {path}")
    if file_sha256(staged) != file_sha256(dmg):
        raise RuntimeError("The staged DMG is not the verified dist/ DMG")
    checks = [
        ["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)],
        ["codesign", "--verify", "--strict", "--verbose=2", str(dmg)],
        ["xcrun", "stapler", "validate", str(dmg)],
        ["spctl", "-a", "-vvv", "-t", "install", str(dmg)],
    ]
    outputs = []
    for argv in checks:
        result = subprocess.run(argv, capture_output=True, text=True)
        print("$ " + " ".join(argv) + "\n" + result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError(f"{argv[0]} {argv[1]} failed (exit {result.returncode})")
        outputs.append(result.stdout + result.stderr)
    if "source=Notarized Developer ID" not in outputs[-1]:
        raise RuntimeError("Gatekeeper did not accept the DMG as Notarized Developer ID")
    details = subprocess.run(["codesign", "-dvv", str(app)], capture_output=True, text=True)
    text = details.stdout + details.stderr
    if details.returncode or not re.search(r"flags=0x[0-9a-f]*\(.*runtime.*\)", text) \
            or "Timestamp=" not in text:
        raise RuntimeError("App signature lacks the hardened runtime or a secure timestamp")
    authority = re.search(r"^Authority=(Developer ID Application: .+)$", text, re.MULTILINE)
    if not authority or authority.group(1) != os.environ.get("MACOS_SIGNING_IDENTITY", "").strip():
        raise RuntimeError("App is not signed by the imported Developer ID identity")
    try:
        receipt = json.loads((ROOT / "ci-results/notarization.json").read_text())
    except (OSError, ValueError):
        raise RuntimeError("Notarization receipt missing") from None
    if receipt.get("status") != "Accepted":
        raise RuntimeError(f"Notarization status is {receipt.get('status')!r}, not Accepted")
    job_summary([f"### {name} DMG",
                 f"- Identity: `{authority.group(1)}`",
                 f"- Notarization submission: `{receipt.get('id')}` ({receipt['status']})",
                 "- Stapled, Gatekeeper: accepted (Notarized Developer ID); hardened runtime + timestamp"])


def release_verify(name):
    kind = SIGNED_RELEASE_TARGETS.get(name)
    if kind == "android":
        verify_android_release(name)
    elif kind == "macos":
        verify_macos_release(name)
    else:
        raise RuntimeError(f"{name} has no release signature to verify")


def check_release_assets(directory, expected):
    """Exactly the expected public files; never signing material."""
    found = sorted(p.name for p in directory.iterdir())
    for path in directory.iterdir():
        if path.is_symlink() or not path.is_file():
            raise RuntimeError(f"Release assets must be regular files: {path.name}")
        if path.name.lower().endswith(SIGNING_MATERIAL_SUFFIXES):
            raise RuntimeError(f"Refusing signing material in release assets: {path.name}")
    if found != sorted(expected):
        raise RuntimeError(f"Release assets {found} differ from the expected {sorted(expected)}")


def release_stage(name):
    dist = ROOT / "artifacts" / name / "dist"
    out = ROOT / "release-assets" / name
    if out.exists():
        raise RuntimeError("Refusing stale release-assets directory")
    out.mkdir(parents=True)
    for source, asset in RELEASE_ASSETS[name].items():
        path = dist / source
        if path.is_symlink() or not path.is_file():
            raise RuntimeError(f"{name}: expected release output missing: {path}")
        shutil.copyfile(path, out / asset)
    check_release_assets(out, RELEASE_ASSETS[name].values())
    job_summary([f"### {name} assets"] + [f"- `{asset}` {file_sha256(out / asset)}"
                                         for asset in sorted(RELEASE_ASSETS[name].values())])


def release_checksums(directory, targets):
    """Validate the merged asset set and write SHA256SUMS (sha256sum format)."""
    directory = Path(directory)
    expected = [asset for target in targets for asset in RELEASE_ASSETS[target].values()]
    sums = directory / "SHA256SUMS"
    if sums.exists():
        raise RuntimeError("Refusing stale SHA256SUMS")
    check_release_assets(directory, expected)
    sums.write_text("".join(f"{file_sha256(directory / asset)}  {asset}\n" for asset in sorted(expected)))
    return sums


def release_cleanup():
    """always(): delete every piece of signing material this job could hold."""
    removed = []
    try:
        directory = release_signing_dir()
    except RuntimeError:
        directory = None
    if directory is not None:
        keychain = directory / KEYCHAIN_NAME
        if platform.system() == "Darwin" and keychain.exists():
            subprocess.run(["security", "delete-keychain", str(keychain)], check=False)
            removed.append("temporary keychain")
        if directory.exists():
            shutil.rmtree(directory)
            removed.append(str(directory))
    # build.py writes the store/key passwords into the generated Gradle signing
    # config; the generated project is never uploaded, but remove it anyway.
    dx = ROOT / "target" / "dx"
    if dx.is_dir():
        for gradle in dx.glob("*/*/android/app/app/build.gradle.kts"):
            if "storePassword" in gradle.read_text(encoding="utf-8", errors="replace"):
                gradle.unlink()
                removed.append(str(gradle.relative_to(ROOT)))
    print("Release signing cleanup: " + (", ".join(removed) if removed else "nothing to remove"))


def main():
    configure_utf8_console()
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["bootstrap", "build", "release-version", "release-android-signing",
                                           "release-macos-signing", "release-build", "release-verify",
                                           "release-stage", "release-checksums", "release-cleanup"])
    parser.add_argument("--target", choices=[p["target"] for p in PLATFORMS], default=os.environ.get("CI_FRONTEND_TARGET"))
    parser.add_argument("--profile", choices=["debug", "release"], default=os.environ.get("CI_FRONTEND_PROFILE", "release"))
    parser.add_argument("--tag", help="release-version: the pushed tag, e.g. v0.3.1b36")
    parser.add_argument("--assets", help="release-checksums: directory holding every downloaded asset")
    parser.add_argument("--release-targets", help="release-checksums: comma-separated targets that were built")
    args = parser.parse_args()
    if args.action in {"bootstrap", "build", "release-build", "release-verify", "release-stage"} and not args.target:
        parser.error(f"{args.action} needs --target or CI_FRONTEND_TARGET")
    if args.action == "bootstrap":
        bootstrap(args.target)
    elif args.action == "build":
        run(args.target, args.profile)
    elif args.action == "release-version":
        info = release_version(args.tag)
        github_file("GITHUB_OUTPUT", info)
        job_summary([f"## {info['name']}", f"Tag `{info['tag']}`, Android versionCode {info['version_code']}."])
    elif args.action == "release-android-signing":
        prepare_android_release_signing()
    elif args.action == "release-macos-signing":
        prepare_macos_release_signing()
    elif args.action == "release-build":
        release_build(args.target)
    elif args.action == "release-verify":
        release_verify(args.target)
    elif args.action == "release-stage":
        release_stage(args.target)
    elif args.action == "release-checksums":
        names = [n for n in (args.release_targets or "").split(",") if n]
        if not args.assets or not names or any(n not in RELEASE_TARGETS for n in names):
            parser.error("release-checksums needs --assets and --release-targets from " + ",".join(RELEASE_TARGETS))
        print(release_checksums(args.assets, names).read_text(), end="")
    else:
        release_cleanup()

if __name__ == "__main__":
    main()

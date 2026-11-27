"""Frontend CI driver.

`bootstrap`/`build` are build-only CI: Android uses ephemeral development
signing and never production credentials. The `release-*` actions run only in
the tag-triggered release workflow (.github/workflows/release.yml), which is
the one place production signing material (GitHub secrets) is used, and in
the manual promote workflow (.github/workflows/promote.yml), which uses only
the store API keys to move an existing build between testing tracks/groups.
"""
from __future__ import annotations

import argparse
import base64
import datetime
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import plistlib
import re
import secrets
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
import urllib.parse
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
# bundletool builds the universal APK unsigned on the release path; the
# `--final` step then signs it with the upload key (apksigner) and
# release-verify requires exactly that one signer. Only this exact line, and
# only in a final-signing Android release build, is reported instead of fatal.
BUNDLETOOL_UNSIGNED_INTERMEDIATE = re.compile(
    r"^WARNING: The APKs won't be signed and thus not installable unless you also pass a keystore via the flag --ks\. "
    r"See the command help for more information\.$"
)
# jarsigner signs the Play AAB with the upload key, whose certificate is
# self-signed (Google re-signs the app). It says so under a bare "Warning:"
# header, with a note on the entries it cannot cover. Only that exact block,
# and only in the Play build, is reported instead of fatal: an expiring
# certificate or any other text in the block stays fatal.
JARSIGNER_HEADER = "Warning:"
JARSIGNER_SELF_SIGNED = "The signer's certificate is self-signed."
JARSIGNER_POSIX = ("POSIX file permission and/or symlink attributes detected. These attributes are ignored when signing "
                   "and are not protected by the signature.")
# Xcode 27's own build-tool notices, independent of this project's code: the
# App Intents extractor says it skipped a target that does not link
# AppIntents, and xcodebuild asserts about simulator devices installed on the
# runner image (each assertion block ends with the "file a bug" line). Exact
# texts only; reported separately, every other warning stays fatal.
XCODE_APPINTENTS_SKIPPED = re.compile(
    r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3} appintentsmetadataprocessor\[\d+:\d+\] "
    r"warning: Metadata extraction skipped, no AppIntents\.framework dependency found$"
)
XCODE_SIMULATOR_ASSERTION = re.compile(
    r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3} xcodebuild\[\d+:\d+\] \[MT\] DVTAssertions: "
    r"Warning in DVTiOSFrameworks/IDEiOSSupportCore/DVTiPhoneSimulator\.m:\d+$"
)
XCODE_ASSERTION_FOOTER = (
    "Please file a bug at https://feedbackassistant.apple.com with this warning message "
    "and any useful information you can provide."
)
WARNING = re.compile(r"(?i)(?:(?:^|\s)(?:warning(?:\[[^]]+\])?:|warn(?:ing)?\s+(?!0\b)\S|w:\s)|::warning(?: [^:]*)?::)")

# Only the two observed Cargo/libcurl error-55 SSL transport diagnostics may
# trigger a clean offline verification after an otherwise successful fetch.
CARGO_FETCH_TRANSIENT = re.compile(
    r"^warning: spurious network error \([1-3] tries remaining\): \[55\] "
    r"Failed sending data to the peer \(OpenSSL SSL_read: "
    r"(?:SSL_ERROR_SYSCALL|OpenSSL/[0-9.]+: error:0A000126:SSL routines::unexpected eof while reading), errno 0\)$"
)

# Gradle prints the cause tree of each failure under "* What went wrong:". A
# build may be retried only when every root cause in every failure is one of
# these download answers: rate limited (429), server error (5xx) or a timeout.
# Anything else (a missing artifact, TLS trust, a compile error) is final.
GRADLE_FAILURE = re.compile(r"^FAILURE: Build (?:failed with an exception|completed with [0-9]+ failures)\.$")
GRADLE_REQUEST = re.compile(r"^Could not (?:GET|HEAD|get resource|download|head) ")
GRADLE_TRANSIENT_STATUS = re.compile(
    r"^Could not (?:GET|HEAD) 'https://[^'\s]+'\. Received status code (?:429|5[0-9]{2}) from server(?:: .*)?$")
GRADLE_TRANSIENT_TIMEOUT = re.compile(
    r"^(?:Read timed out|[Cc]onnect timed out|Connection timed out"
    r"|Connect to \S+ \[[^\]]*\] failed: (?:Read timed out|[Cc]onnect timed out|Connection timed out(?: \(Connection timed out\))?))$")
# Consequences Gradle prints beside the first cause; never a cause on their own.
GRADLE_REPEATED_CAUSE = re.compile(r"^(?:There (?:is|are) [0-9]+ more failures? with (?:an )?identical causes?\.|Skipped due to earlier error)$")
GRADLE_WRAPPER_TRANSIENT = re.compile(
    r"^Exception in thread \"main\" java\.io\.IOException: "
    r"(?:Server returned HTTP response code: (?:429|5[0-9]{2}) for URL: https://services\.gradle\.org/\S+"
    r"|Downloading from https://services\.gradle\.org/\S+ failed: timeout \([0-9]+ms\))$")
COMPILER_ERROR = re.compile(r"^(?:error(?:\[E[0-9]+\])?: |e: (?:file:|/))|could not compile `")
# Waits before each further attempt of a build that could not download. Maven
# Central has refused a hosted runner for longer than five minutes.
DEPENDENCY_RETRY_DELAYS = (60, 240, 600, 900)
MAVEN_CENTRAL_MIRROR_SCRIPT = Path(__file__).resolve().parent / "maven-central-mirror.gradle"


def transient_dependency_failures(text):
    """Root causes when a failed build is only a transient dependency download.

    Empty means "not recognised": the failure is final."""
    lines = [ANSI.sub("", line).rstrip() for line in text.splitlines()]
    if any(COMPILER_ERROR.search(line.strip()) for line in lines):
        return []
    wrapper = [line.strip() for line in lines if GRADLE_WRAPPER_TRANSIENT.fullmatch(line.strip())]
    if any(line.strip().startswith("Exception in thread ") and line.strip() not in wrapper for line in lines):
        return []
    headers = sum(bool(GRADLE_FAILURE.fullmatch(line.strip())) for line in lines)
    blocks = [index for index, line in enumerate(lines) if line.strip() == "* What went wrong:"]
    if bool(headers) != bool(blocks) or len(blocks) < headers:
        return []
    found = list(wrapper)
    for start in blocks:
        tree = []
        for line in lines[start + 1:]:
            if line.strip().startswith("* ") or GRADLE_FAILURE.fullmatch(line.strip()):
                break
            cause = re.match(r"^( *)> (.*)$", line)
            if cause:
                tree.append((len(cause.group(1)), cause.group(2).strip()))
        if not tree:
            return []
        transient = []
        path = []
        for index, (depth, text_line) in enumerate(tree):
            path = [row for row in path if row[0] < depth]
            leaf = index + 1 == len(tree) or tree[index + 1][0] <= depth
            if leaf:
                requested = any(GRADLE_REQUEST.match(parent) for _, parent in path)
                if (GRADLE_TRANSIENT_STATUS.fullmatch(text_line)
                        or (requested and GRADLE_TRANSIENT_TIMEOUT.fullmatch(text_line))):
                    transient.append(text_line)
                elif not GRADLE_REPEATED_CAUSE.fullmatch(text_line):
                    return []
            path.append((depth, text_line))
        if not transient:
            return []
        found += transient
    return found


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


def checked(argv, env, label, *, dioxus_progress=False, android_upstream_r8=False, android_final_signing=False,
            android_store_signing=False, receipt_extra=None):
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
    unsigned_intermediate_lines = []
    self_signed_lines = []
    xcode_tool_lines = []
    block = None  # lines of an open jarsigner notice

    def close_block():
        nonlocal block
        if block is not None:
            (self_signed_lines if JARSIGNER_SELF_SIGNED in block else warnings).extend(block)
            block = None

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
                if android_store_signing and plain == JARSIGNER_HEADER:
                    close_block()
                    block = [plain]
                    continue
                if block is not None and plain in (JARSIGNER_SELF_SIGNED, JARSIGNER_POSIX):
                    block.append(plain)
                    continue
                if block is not None and plain:
                    block = [entry for entry in block if entry != JARSIGNER_SELF_SIGNED] + [plain]
                close_block()
                if dioxus_progress and DIOXUS_METADATA_PROGRESS.fullmatch(plain):
                    progress_lines.append(plain)
                elif android_upstream_r8 and ANDROID_R8_UPSTREAM_RULE_NOTICE.fullmatch(plain):
                    upstream_r8_lines.append(plain)
                elif android_final_signing and BUNDLETOOL_UNSIGNED_INTERMEDIATE.fullmatch(plain):
                    unsigned_intermediate_lines.append(plain)
                elif (XCODE_APPINTENTS_SKIPPED.fullmatch(plain) or XCODE_SIMULATOR_ASSERTION.fullmatch(plain)
                      or (plain == XCODE_ASSERTION_FOOTER and any(XCODE_SIMULATOR_ASSERTION.fullmatch(seen)
                                                                   for seen in xcode_tool_lines))):
                    xcode_tool_lines.append(plain)
                else:
                    if WARNING.search(plain):
                        warnings.append(plain)
        close_block()
        result = child.wait()
    (logs / (label + ".json")).write_text(json.dumps({"argv": argv, "resolved_argv": resolved, "exit": result, "warning_lines": len(warnings), "external_metadata_progress_warnings": progress_lines, "external_android_r8_rule_notices": upstream_r8_lines, "bundletool_unsigned_intermediate_notices": unsigned_intermediate_lines, "jarsigner_self_signed_notices": self_signed_lines, "external_xcode_tool_notices": xcode_tool_lines, **(receipt_extra or {})}, indent=2) + "\n")
    if result or warnings:
        raise CommandFailure(label, result, warnings)


def install_maven_central_mirror():
    """Route the CI Gradle home's Maven Central requests to Google's mirror.

    Maven Central has refused a hosted runner (HTTP 429) through every
    dependency retry, about 30 minutes in all; the mirror serves the same files.
    build.py always builds with ROOT/.gradle-user-home, and Gradle reads every
    init script in its init.d before evaluating any build."""
    init_d = ROOT / ".gradle-user-home" / "init.d"
    init_d.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(MAVEN_CENTRAL_MIRROR_SCRIPT, init_d / MAVEN_CENTRAL_MIRROR_SCRIPT.name)


def checked_gradle_build(argv, env, label, **options):
    """`checked`, repeated (bounded) only after a transient dependency download.

    A warning in any attempt, or a failure with any other root cause, is final
    at once. Each attempt keeps its own log and receipt; a retry's receipt and
    `<label>-dependency-retries.json` name the failures it follows."""
    install_maven_central_mirror()
    retries = []
    for attempt in range(1, len(DEPENDENCY_RETRY_DELAYS) + 2):
        current = label if attempt == 1 else f"{label}-dependency-retry-{attempt - 1}"
        extra = {"receipt_extra": {"dependency_retry_attempt": attempt - 1, "after_transient_dependency_failures": retries}} if retries else {}
        try:
            checked(argv, env, current, **options, **extra)
            return
        except CommandFailure as failure:
            causes = []
            if failure.result and not failure.warnings:
                log = ROOT / "ci-results" / (current + ".log")
                causes = transient_dependency_failures(log.read_text(encoding="utf-8", errors="replace"))
            if not causes or attempt > len(DEPENDENCY_RETRY_DELAYS):
                raise
            delay = DEPENDENCY_RETRY_DELAYS[attempt - 1]
            retries.append({"attempt": current, "exit": failure.result, "transient_causes": len(causes),
                            "first_causes": causes[:10], "retry_after_s": delay})
            (ROOT / "ci-results" / (label + "-dependency-retries.json")).write_text(json.dumps(retries, indent=2) + "\n")
            job_summary([f"### {label}: dependency download retry {attempt}",
                         f"- {len(causes)} transient download failure(s), retrying the build in {delay} s",
                         f"- First: `{causes[0]}`"])
            time.sleep(delay)


def fetch_application(env, label="application-fetch"):
    """Require a clean fetch, or verify an SSL-recovered fetch fully offline."""
    try:
        checked(["cargo", "fetch", "--locked"], env, label)
    except CommandFailure as failure:
        if (failure.result != 0 or not failure.warnings
                or not all(CARGO_FETCH_TRANSIENT.fullmatch(line) for line in failure.warnings)):
            raise
        # Keep the initial log/receipt unchanged. No second network attempt and
        # no warning exemption in the verification or subsequent build steps.
        checked(["cargo", "fetch", "--locked", "--offline"], env,
                label + "-offline-verification")


def ci_env():
    env = os.environ.copy()
    env.update(PYTHONUTF8="1", PYTHONIOENCODING="utf-8")
    overrides = [name for name in env
                 if name in {"CARGO_ENCODED_RUSTFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS"}
                 or (name.startswith("CARGO_TARGET_") and name.endswith("_RUSTFLAGS"))]
    if overrides:
        raise RuntimeError(f"CI warning flags require a clean runner environment; remove {sorted(overrides)}")
    # Two by default (the GitLab Docker runners are shared); CI_CARGO_JOBS=cores
    # lets a dedicated hosted runner use every core it has.
    jobs = str(os.cpu_count() or 2) if env.get("CI_CARGO_JOBS") == "cores" else "2"
    env.update({"RUSTUP_TOOLCHAIN": RUST, "CARGO_BUILD_JOBS": jobs, "CARGO_INCREMENTAL": "0",
                "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0", "RUSTFLAGS": "-D warnings",
                "RUSTDOCFLAGS": "-D warnings", "CARGO_TERM_COLOR": "never", "NO_COLOR": "1"})
    # Never forward local deployment credentials into build-only jobs.
    for key in list(env):
        if key.startswith(("NOTARY_", "IOS_DISTRIBUTION_", "IOS_DEVELOPMENT_", "ANDROID_KEY", "ANDROID_UPLOAD_",
                           "SEDS_GS_CI_ANDROID_", "SEDS_GS_RELEASE_", "SEDS_GS_NOTARY_", "MACOS_SIGNING_",
                           "MACOS_DEVELOPER_ID_", "APPLE_NOTARY_", "APPLE_APP_SPECIFIC_",
                           "IOS_APPSTORE_", "PLAY_SERVICE_ACCOUNT_")) or key in {"APPLE_ID", "APPLE_TEAM_ID"}:
            env.pop(key)
    bins = [ROOT / ".ci-tools/dx/bin", ROOT / ".ci-tools/wasm-bindgen", ROOT / ".ci-tools/binaryen/bin", ROOT / ".ci-tools/linuxdeploy/bin", ROOT / ".ci-tools/node/node_modules/.bin"]
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


def install_dioxus_cli(host, version, env):
    """dx into .ci-tools/dx. Compiling it takes 20-35 minutes, so build-only CI
    restores a previous copy from the Actions cache; it is used only if it
    reports exactly the version the application lock resolved, otherwise it is
    rebuilt. Platform CI saves it; release.yml only restores it."""
    root = ROOT / ".ci-tools/dx"
    if (ROOT / ".ci-tools").is_symlink() or root.is_symlink():
        raise RuntimeError("Refusing linked CI tools directory")
    dx = root / "bin" / ("dx.exe" if host[0] == "Windows" else "dx")
    reported = ""
    if dx.is_file():
        result = subprocess.run([str(dx), "--version"], capture_output=True, text=True, env=env)
        reported = result.stdout.strip() if result.returncode == 0 else ""
    reused = re.fullmatch(rf"dioxus {re.escape(version)}(\s.*)?", reported) is not None
    if not reused:
        if root.exists():
            shutil.rmtree(root)
        # Exact CLI version; it resolves its own dependencies afresh, matching
        # the app bootstrap policy for published CLI locks with yanked crates.
        # Explicit selection avoids Cargo's implicit-toolchain warning.
        checked(["cargo", "+" + RUST, "install", "dioxus-cli", "--version", "=" + version, "--root", str(root)],
                env, "dioxus-cli")
    receipt = {"version": version, "reused_from_cache": reused, "reported": reported or None,
               "binary_sha256": file_sha256(dx) if dx.is_file() else None}
    (ROOT / "ci-results/dioxus-cli.json").write_text(json.dumps(receipt, indent=2) + "\n")
    github_file("GITHUB_OUTPUT", {"dx-built": "false" if reused else "true", "dx-version": f"{RUST}-{version}"})


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
    # GitLab's Docker jobs run as root, and pip warns about that on every install.
    checked([sys.executable, "-m", "pip", "install", "--root-user-action=ignore", "--disable-pip-version-check",
             "Pillow==12.0.0"], env, "pillow")
    versions = resolved_tool_versions()
    # Resolve/download the exact application lock before dx's timer-based
    # metadata spinner; subsequent application Cargo work is offline/locked.
    fetch_application(env)
    wasm_bindgen_asset(host, versions["wasm-bindgen"])
    receipt = ROOT / "ci-results/resolved-tools.json"
    receipt.parent.mkdir(exist_ok=True)
    receipt.write_text(json.dumps({"versions": versions,
                                  "lock_sha256": file_sha256(ROOT / "Cargo.lock")}, indent=2) + "\n")
    install_dioxus_cli(host, versions["dioxus"], env)
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


def android_page_alignment(packages, *, require_native, release=False):
    """Google Play: 16 KB page support for every 64-bit native library
    (scripts/check_android_alignment.py). `release` also requires stripped
    libraries and, in an AAB, their native debug symbols. Fails the step on
    any problem."""
    import importlib.util
    spec = importlib.util.spec_from_file_location("check_android_alignment",
                                                  Path(__file__).resolve().parents[1] / "check_android_alignment.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    packages = [Path(p) for p in packages]
    if not packages:
        raise RuntimeError("No Android package to check for 16 KB page alignment")
    argv = (["--require-native"] if require_native else []) + (["--release"] if release else []) + [str(p) for p in packages]
    if module.main(argv):
        raise RuntimeError("Android package failed the Google Play native-code checks (16 KB page alignment"
                           + (", stripped libraries, native debug symbols" if release else "") + "): "
                           + ", ".join(p.name for p in packages))


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
    build = checked
    if android_signed(name):
        # Gradle downloads: dependency retry, and the cached home (platforms.yml).
        env["GRADLE_USER_HOME"] = str(ROOT / ".gradle-user-home")
        build = checked_gradle_build
    build(build_command(name, profile), env, "build-" + name + "-" + profile, dioxus_progress=True,
          android_upstream_r8=name.startswith("android") and profile == "release")
    output = ROOT / "artifacts" / name
    files = sorted(p for p in output.rglob("*") if p.is_file())
    if not files:
        raise RuntimeError("Build produced no staged artifacts; not a successful platform build")
    if android_signed(name):
        # Wear OS is Kotlin only; the phone app must carry its Rust library.
        # Debug builds keep their symbols on purpose (the template's keepDebugSymbols).
        android_page_alignment([p for p in files if p.suffix in {".apk", ".aab"}], require_native=not is_wear(name),
                               release=name.startswith("android") and profile == "release")
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
# Signing kind per store build target (release_env); not GitHub Release assets.
STORE_SIGNING = {"ios": "ios", "android": "android"}
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
    "ios": ("IOS_DISTRIBUTION_MOBILEPROVISION",),
}
ANDROID_RELEASE_SECRETS = ("ANDROID_UPLOAD_KEYSTORE_B64", "ANDROID_KEYSTORE_PASSWORD")
MACOS_CERT_SECRETS = ("MACOS_DEVELOPER_ID_P12_B64", "MACOS_DEVELOPER_ID_P12_PASSWORD")
NOTARY_API_SECRETS = ("APPLE_NOTARY_KEY_P8_B64", "APPLE_NOTARY_KEY_ID", "APPLE_NOTARY_ISSUER_ID")
NOTARY_APPLE_ID_SECRETS = ("APPLE_ID", "APPLE_APP_SPECIFIC_PASSWORD", "APPLE_TEAM_ID")
NOTARY_PROFILE_NAME = "seds-gs-release"
KEYCHAIN_NAME = "release.keychain-db"
# The release workflow refuses a tag whose commit is not on one of these
# branches (after fetching them), and any run whose ref GitHub does not report
# as protected (rulesets in docs/releasing.md).
RELEASE_BRANCHES = ("main", "dev")
# Store uploads. They go ONLY to TestFlight (an App Store Connect build, never
# submitted for App Store review; optionally added to named beta groups, which
# for external groups means a *beta* app review) and to a Play testing track:
# internal, alpha (closed) or beta (open). Nothing here can create a public App
# Store or Play production release.
IOS_BUNDLE_ID = "com.UBSEDS.GS26"
PLAY_TRACK = "internal"                       # default testing track
PLAY_TRACKS = ("internal", "alpha", "beta")   # promotion order; never "production"
PLAY_NOTES_LIMIT = 500
PLAY_API = "https://androidpublisher.googleapis.com/androidpublisher/v3/applications"
PLAY_UPLOAD_API = "https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications"
PLAY_SCOPE = "https://www.googleapis.com/auth/androidpublisher"
GOOGLE_TOKEN_URI = "https://oauth2.googleapis.com/token"
IOS_STORE_SECRETS = ("IOS_DISTRIBUTION_P12_B64", "IOS_DISTRIBUTION_P12_PASSWORD", "IOS_APPSTORE_PROFILE_B64")
PLAY_STORE_SECRETS = ("PLAY_SERVICE_ACCOUNT_JSON_B64",)
# store -> (build target, secrets whose presence enables the store, every secret it then needs)
STORES = {
    "testflight": ("ios", IOS_STORE_SECRETS, IOS_STORE_SECRETS + NOTARY_API_SECRETS),
    "play": ("android", PLAY_STORE_SECRETS, PLAY_STORE_SECRETS + ANDROID_RELEASE_SECRETS),
    # promote.yml: no build, so no signing material beyond the store API keys.
    "testflight-promote": ("ios", NOTARY_API_SECRETS, NOTARY_API_SECRETS),
    "play-promote": ("android", PLAY_STORE_SECRETS, PLAY_STORE_SECRETS),
}
STORE_OUTPUTS = {"ios": Path("dist/ipas/UBSEDS GS.ipa"), "android": Path("dist/SEDS GS.aab")}


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


EXPIRY_WARNING_DAYS = 30


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None)


def cert_not_after(data, inform="DER"):
    """notAfter (naive UTC) of a DER or PEM certificate, or None if unreadable."""
    result = subprocess.run(["openssl", "x509", "-inform", inform, "-noout", "-enddate"], input=data,
                            capture_output=True)
    match = re.fullmatch(rb"notAfter=(.+?) GMT\s*", result.stdout) if not result.returncode else None
    if not match:
        return None
    try:
        return datetime.datetime.strptime(match.group(1).decode(), "%b %d %H:%M:%S %Y")
    except ValueError:
        return None


def report_expiry(label, expires, now=None):
    """Job-summary line for a signing credential's expiry; a ::warning:: (never
    a failure) when it is unknown or under EXPIRY_WARNING_DAYS away."""
    if expires is None:
        print(f"::warning title=Signing expiry::Could not read the expiry of the {label}", flush=True)
        job_summary([f"- {label}: expiry unknown"])
        return None
    days = (expires - (now or utc_now())).days
    if days < EXPIRY_WARNING_DAYS:
        state = "EXPIRED" if expires <= (now or utc_now()) else f"expires in {days} days"
        print(f"::warning title=Signing expiry::The {label} {state} ({expires:%Y-%m-%d}). Renew it and update "
              "the secret; see docs/releasing.md.", flush=True)
    job_summary([f"- {label}: valid until {expires:%Y-%m-%d} ({days} days)"])
    return days


def keychain_cert_not_after(keychain, identity):
    try:
        pem = quiet_run(["security", "find-certificate", "-c", identity, "-p", str(keychain)],
                        "Reading the signing certificate", capture=True)
    except RuntimeError:
        return None
    return cert_not_after(pem, "PEM")


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
    report_expiry("Android upload certificate", cert_not_after(der))


def colon_hex(digest):
    digest = digest.upper()
    return ":".join(digest[i:i + 2] for i in range(0, len(digest), 2))


def normalized_digest(value):
    return re.sub(r"[^0-9a-f]", "", (value or "").lower())


def security_keychains():
    out = quiet_run(["security", "list-keychains", "-d", "user"], "Listing keychains", capture=True)
    return [line.strip().strip('"') for line in out.decode("utf-8", errors="replace").splitlines() if line.strip()]


def codesigning_identity_hashes(keychain):
    """[(SHA-1 of the certificate, identity name)] valid for code signing."""
    out = quiet_run(["security", "find-identity", "-v", "-p", "codesigning", str(keychain)],
                    "Listing code signing identities", capture=True).decode("utf-8", errors="replace")
    return re.findall(r'^\s*\d+\)\s+([0-9A-F]{40})\s+"([^"]+)"\s*$', out, re.MULTILINE)


def codesigning_identities(keychain):
    return [name for _sha1, name in codesigning_identity_hashes(keychain)]


def notary_mode():
    api = [name for name in NOTARY_API_SECRETS if os.environ.get(name, "").strip()]
    apple_id = [name for name in NOTARY_APPLE_ID_SECRETS if os.environ.get(name, "").strip()]
    if len(api) == len(NOTARY_API_SECRETS):
        return "api-key"
    if api:
        require_secrets(NOTARY_API_SECRETS, "Notarization with an App Store Connect API key")
    if len(apple_id) == len(NOTARY_APPLE_ID_SECRETS):
        return "apple-id"
    # Name the preferred App Store Connect API key set first, whatever subset
    # of the Apple ID fallback happens to exist (e.g. only APPLE_TEAM_ID).
    fallback = [name for name in NOTARY_APPLE_ID_SECRETS if name not in apple_id]
    raise RuntimeError("Apple notarization: missing GitHub secrets APPLE_NOTARY_KEY_P8_B64, APPLE_NOTARY_KEY_ID "
                       "and APPLE_NOTARY_ISSUER_ID (the preferred App Store Connect API key). Fallback only: "
                       "an Apple ID, which also needs " + ", ".join(fallback) + ". See docs/releasing.md. "
                       "A DMG is never released without notarization.")


def create_release_keychain(directory, p12_secret, password_secret, p12_name, what):
    """A temporary keychain (random password) in the job-local signing
    directory holding one imported .p12, first in the user search list.
    `release-cleanup` deletes it, which also drops it from the search list."""
    keychain = directory / KEYCHAIN_NAME
    password = secrets.token_urlsafe(32)
    print(f"::add-mask::{password}", flush=True)
    quiet_run(["security", "create-keychain", "-p", password, str(keychain)], "Creating the temporary keychain")
    quiet_run(["security", "set-keychain-settings", "-lut", "21600", str(keychain)], "Setting the keychain timeout")
    quiet_run(["security", "unlock-keychain", "-p", password, str(keychain)], "Unlocking the temporary keychain")
    p12 = directory / p12_name
    private_write(p12, decode_secret(p12_secret))
    try:
        quiet_run(["security", "import", str(p12), "-k", str(keychain), "-f", "pkcs12",
                   "-P", os.environ[password_secret],
                   "-T", "/usr/bin/codesign", "-T", "/usr/bin/security"], f"Importing the {what} certificate")
    finally:
        p12.unlink(missing_ok=True)
    quiet_run(["security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s",
               "-k", password, str(keychain)], "Allowing codesign to use the key")
    others = [path for path in security_keychains() if Path(path) != keychain]
    quiet_run(["security", "list-keychains", "-d", "user", "-s", str(keychain), *others],
              "Adding the temporary keychain to the search list")
    return keychain


def prepare_macos_release_signing():
    require_secrets(MACOS_CERT_SECRETS, "Developer ID signing")
    mode = notary_mode()
    directory = release_signing_dir(create=True)
    keychain = create_release_keychain(directory, "MACOS_DEVELOPER_ID_P12_B64", "MACOS_DEVELOPER_ID_P12_PASSWORD",
                                       "developer-id.p12", "Developer ID")
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
    report_expiry("Developer ID Application certificate", keychain_cert_not_after(keychain, identity))
    job_summary([f"macOS signing identity: `{identity}`",
                 f"Notarization credentials: {'App Store Connect API key' if mode == 'api-key' else 'Apple ID (app-specific password)'}"])


def release_env(name):
    """ci_env() (which drops every signing variable), plus exactly the signing
    variables build.py needs for this target's platform, and nothing else."""
    kind = SIGNED_RELEASE_TARGETS.get(name) or STORE_SIGNING.get(name)
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
    elif kind == "ios":
        profile = env.get("IOS_DISTRIBUTION_MOBILEPROVISION")
        if not profile:
            raise RuntimeError("App Store signing is not prepared (missing IOS_DISTRIBUTION_MOBILEPROVISION); "
                               "run release-ios-signing with the GitHub secrets first")
        if Path(profile).resolve().parent != release_signing_dir():
            raise RuntimeError("The App Store profile must be the job-local file written by release-ios-signing")
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
    android = name.startswith("android")
    build = checked_gradle_build if android else checked
    build(release_command(name), env, "release-" + name, dioxus_progress=True, android_upstream_r8=android,
          android_final_signing=android and "--final" in release_command(name))


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
    """(DN, SHA-256) for each distinct signer in `apksigner verify --print-certs` output.

    Older build-tools label signers `Signer #N`; newer ones label them by scheme,
    e.g. `V3.0 Signer:` or `V3.1 Signer #N:`, and may list one signer under
    several schemes. Each label's DN pairs with its own digest, and signers are
    de-duplicated; the distinct set must equal `Number of signers`."""
    count = re.search(r"^Number of signers: (\d+)$", output, re.MULTILINE)
    label = r"((?:V\d+(?:\.\d+)? )?Signer(?: #\d+)?):?"
    digests = dict(re.findall(rf"^{label} certificate SHA-256 digest: ([0-9a-f]{{64}})$", output, re.MULTILINE))
    names = dict(re.findall(rf"^{label} certificate DN: (.+)$", output, re.MULTILINE))
    if not count or not digests or set(digests) != set(names):
        raise RuntimeError("Could not read the APK signer certificates from apksigner output")
    signers = []
    for key in digests:
        pair = (names[key], digests[key])
        if pair not in signers:
            signers.append(pair)
    if int(count.group(1)) != len(signers):
        raise RuntimeError("Could not read the APK signer certificates from apksigner output")
    return signers


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
    android_page_alignment([apk], require_native=True, release=True)
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


# ---------------------------------------------------------------------------
# Protected-ref guard: every signing/publishing job depends on it.

def git_capture(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()


def release_guard(tag, branches=RELEASE_BRANCHES):
    """Refuse a release unless it was started from a protected ref (the pushed
    tag, or a workflow_dispatch from a release branch) and the tag's commit is
    on one of the release branches. Returns the first branch containing it."""
    release_version_tag = RELEASE_TAG.fullmatch(tag or "")
    if not release_version_tag:
        raise RuntimeError(f"Not a release tag: {tag!r}")
    event = os.environ.get("GITHUB_EVENT_NAME", "")
    ref = os.environ.get("GITHUB_REF", "")
    if event == "push":
        if ref != f"refs/tags/{tag}":
            raise RuntimeError(f"Tag push for {tag} arrived with ref {ref!r}")
    elif event == "workflow_dispatch":
        if ref not in {f"refs/heads/{branch}" for branch in branches}:
            raise RuntimeError(f"Run the release workflow from {' or '.join(branches)}, not {ref!r}")
    else:
        raise RuntimeError(f"Releases run only from a tag push or workflow_dispatch, not {event!r}")
    if os.environ.get("GITHUB_REF_PROTECTED", "").strip().lower() != "true":
        raise RuntimeError(f"{ref} is not a protected ref (GITHUB_REF_PROTECTED is not true). Add the release tag "
                           "and branch rulesets from docs/releasing.md before releasing.")
    dry_run = event == "workflow_dispatch" and os.environ.get("RELEASE_DRY_RUN", "").strip().lower() == "true"
    if dry_run:
        # A no-publish dry run builds the dispatched protected branch head; the tag
        # need not exist yet (pushing it would publish). It still names the version.
        commit = os.environ.get("GITHUB_SHA", "").strip() or git_capture("rev-parse", "HEAD")
        checkout_ref = commit
    else:
        commit = git_capture("rev-parse", "--verify", "--quiet", f"refs/tags/{tag}^{{commit}}")
        checkout_ref = f"refs/tags/{tag}"
    found = []
    for branch in branches:
        fetched = subprocess.run(["git", "fetch", "--quiet", "--no-tags", "origin",
                                  f"+refs/heads/{branch}:refs/remotes/origin/{branch}"],
                                 cwd=ROOT, capture_output=True, text=True)
        if fetched.returncode:
            print(f"origin/{branch} could not be fetched; it does not count as containing {tag}", flush=True)
            continue
        if subprocess.run(["git", "merge-base", "--is-ancestor", commit, f"refs/remotes/origin/{branch}"],
                          cwd=ROOT).returncode == 0:
            found.append(branch)
    if not found:
        raise RuntimeError(f"Tag {tag} ({commit}) is not on {' or '.join('origin/' + b for b in branches)}; "
                           "a release must be tagged on a protected release branch")
    return {"guarded": "true", "release_branch": found[0], "commit": commit, "ref": checkout_ref}


# ---------------------------------------------------------------------------
# Store uploads (TestFlight and a Play testing track). Each store job first
# runs `release-store-check`; with none of its store secrets set it skips with
# a notice and the GitHub Release is unaffected. The check also validates the
# selected Play track / TestFlight groups, so a bad value fails before any build.

STORE_TITLES = {"testflight": "TestFlight upload", "play": "Google Play testing-track upload",
                "testflight-promote": "TestFlight beta-group distribution",
                "play-promote": "Google Play track promotion"}


def play_track(value=None):
    """The Play testing track to use: PLAY_TRACK (a workflow_dispatch input or
    the repository variable), default internal. Only internal, alpha (closed
    testing) and beta (open testing); production and anything else is refused."""
    raw = os.environ.get("PLAY_TRACK", "") if value is None else value
    track = (raw or "").strip().lower() or PLAY_TRACK
    if track == "production":
        raise RuntimeError("PLAY_TRACK=production is refused: the release workflow only uploads to testing tracks "
                           f"({', '.join(PLAY_TRACKS)}). Promote to production by hand in Play Console.")
    if track not in PLAY_TRACKS:
        raise RuntimeError(f"PLAY_TRACK must be one of {', '.join(PLAY_TRACKS)}, got {raw!r}")
    return track


TESTFLIGHT_GROUP_NAME = re.compile(r"[^\x00-\x1f\x7f,]{1,100}")


def testflight_groups(value=None):
    """TESTFLIGHT_GROUPS: comma-separated App Store Connect beta group names
    (input, else repository variable). Empty or "none" means no groups: the
    build reaches only the internal testers, as before."""
    raw = os.environ.get("TESTFLIGHT_GROUPS", "") if value is None else value
    raw = (raw or "").strip()
    if raw.lower() in {"", "none"}:
        return []
    groups = []
    for name in (part.strip() for part in raw.split(",")):
        if not name:
            continue
        if not TESTFLIGHT_GROUP_NAME.fullmatch(name):
            raise RuntimeError(f"TESTFLIGHT_GROUPS has an invalid group name {name!r}")
        if name not in groups:
            groups.append(name)
    if len(groups) > 20:
        raise RuntimeError("TESTFLIGHT_GROUPS names more than 20 groups")
    return groups


def store_options(store):
    if store.startswith("play"):
        return {"track": play_track()}
    return {"groups": ",".join(testflight_groups())}


def store_check(store):
    _target, enabling, required = STORES[store]
    options = store_options(store)
    if not any(os.environ.get(name, "").strip() for name in enabling):
        title = STORE_TITLES[store] + " skipped"
        message = (f"None of {', '.join(enabling)} is set, so this release is not sent to "
                   f"{'TestFlight' if store.startswith('testflight') else 'Google Play'}. "
                   "See docs/releasing.md to enable it.")
        print(f"::notice title={title}::{message}", flush=True)
        job_summary([f"### {title}", message])
        github_file("GITHUB_OUTPUT", {"enabled": "false"})
        return False
    require_secrets(required, STORE_TITLES[store])
    github_file("GITHUB_OUTPUT", {"enabled": "true", **options})
    return True


def mobileprovision_info(path):
    out = quiet_run(["security", "cms", "-D", "-i", str(path)], "Decoding the App Store provisioning profile",
                    capture=True)
    return plistlib.loads(out)


def check_appstore_profile(info, bundle_id=IOS_BUNDLE_ID, now=None):
    """An App Store distribution profile for exactly this bundle id: no device
    list (development/ad hoc), not enterprise, no get-task-allow, unexpired."""
    entitlements = info.get("Entitlements") or {}
    teams = info.get("TeamIdentifier") or []
    team = teams[0] if teams else ""
    if not team or entitlements.get("application-identifier") != f"{team}.{bundle_id}":
        raise RuntimeError(f"IOS_APPSTORE_PROFILE_B64 is not a profile for {bundle_id} "
                           f"(application-identifier {entitlements.get('application-identifier')!r})")
    if info.get("ProvisionedDevices") or info.get("ProvisionsAllDevices"):
        raise RuntimeError("IOS_APPSTORE_PROFILE_B64 is a development, ad hoc or enterprise profile; "
                           "TestFlight needs an App Store distribution profile")
    if entitlements.get("get-task-allow") is True:
        raise RuntimeError("IOS_APPSTORE_PROFILE_B64 allows get-task-allow; it is not an App Store profile")
    expires = info.get("ExpirationDate")
    now = now or utc_now()
    if not isinstance(expires, datetime.datetime) or expires <= now:
        raise RuntimeError(f"IOS_APPSTORE_PROFILE_B64 has expired ({expires})")
    return team


def prepare_ios_release_signing():
    require_secrets(IOS_STORE_SECRETS, "App Store signing")
    directory = release_signing_dir(create=True)
    profile = directory / "appstore.mobileprovision"
    private_write(profile, decode_secret("IOS_APPSTORE_PROFILE_B64"))
    info = mobileprovision_info(profile)
    team = check_appstore_profile(info)
    keychain = create_release_keychain(directory, "IOS_DISTRIBUTION_P12_B64", "IOS_DISTRIBUTION_P12_PASSWORD",
                                       "ios-distribution.p12", "Apple Distribution")
    distribution = [(sha1, name) for sha1, name in codesigning_identity_hashes(keychain)
                    if name.startswith(("Apple Distribution:", "iPhone Distribution:"))]
    if len(distribution) != 1:
        raise RuntimeError("Expected exactly one Apple Distribution identity in IOS_DISTRIBUTION_P12_B64; found "
                           + str(len(distribution)))
    allowed = {hashlib.sha1(bytes(cert)).hexdigest().upper() for cert in info.get("DeveloperCertificates") or []}
    if distribution[0][0] not in allowed:
        raise RuntimeError("The App Store profile does not include the IOS_DISTRIBUTION_P12_B64 certificate; "
                           "regenerate the profile with that certificate")
    github_file("GITHUB_ENV", {"IOS_DISTRIBUTION_MOBILEPROVISION": str(profile)})
    report_expiry("Apple Distribution certificate", keychain_cert_not_after(keychain, distribution[0][1]))
    expires = info.get("ExpirationDate")
    report_expiry("App Store provisioning profile", expires if isinstance(expires, datetime.datetime) else None)
    job_summary([f"App Store signing: `{distribution[0][1]}`, profile `{info.get('Name')}` "
                 f"(team {team}, expires {info.get('ExpirationDate')}). The Apple Watch app is not embedded."])


def store_command(name):
    if name == "ios":
        return [sys.executable, "build.py", "--target", "ios", "--release", "--final"]
    if name == "android":
        return [sys.executable, "build.py", "--target", "android", "--release", "--final", "--package-type", "aab"]
    raise ValueError(f"{name} is not a store target")


def store_build(name):
    validate_host(name)
    env = release_env(name)
    env["CARGO_NET_OFFLINE"] = "true"
    if name == "android":
        verify_bundletool(env)
    for tool in ["dx", "wasm-bindgen", "wasm-opt", "esbuild"]:
        if not shutil.which(tool, path=env["PATH"]):
            raise RuntimeError(f"Missing prepared tool: {tool}")
    if (ROOT / "artifacts" / name).exists():
        raise RuntimeError("Refusing stale platform artifact directory; use a fresh checkout")
    (ROOT / "ci-results").mkdir(exist_ok=True)
    build = checked_gradle_build if name.startswith("android") else checked
    build(store_command(name), env, "store-" + name, dioxus_progress=True,
            android_upstream_r8=name == "android", android_store_signing=name == "android")
    output = ROOT / STORE_OUTPUTS[name]
    if output.is_symlink() or not output.is_file():
        raise RuntimeError(f"Store build produced no {output.relative_to(ROOT)}")


def bundletool_value(aab, xpath):
    jar = ROOT / ".ci-tools/bundletool/bundletool.jar"
    out = subprocess.run(["java", "-jar", str(jar), "dump", "manifest", "--bundle", str(aab), "--xpath", xpath],
                         capture_output=True, text=True)
    if out.returncode:
        raise RuntimeError(f"bundletool dump manifest {xpath} failed:\n{out.stdout}{out.stderr}")
    return out.stdout.strip()


def jar_signer_sha256(output):
    """SHA-256 fingerprints of the signer certificates in `keytool -printcert -jarfile` output."""
    return [normalized_digest(d) for d in re.findall(r"^\s*SHA256:\s*([0-9A-Fa-f:]{95})\s*$", output, re.MULTILINE)]


def verify_store_android(info):
    aab = ROOT / STORE_OUTPUTS["android"]
    env = ci_env()
    verify_bundletool(env)
    jar = subprocess.run(["jarsigner", "-verify", str(aab)], capture_output=True, text=True)
    if jar.returncode or "jar verified." not in jar.stdout or "unsigned entries" in jar.stdout:
        raise RuntimeError("The AAB is not completely signed:\n" + jar.stdout + jar.stderr)
    certs = subprocess.run(["keytool", "-printcert", "-jarfile", str(aab)], capture_output=True, text=True)
    signers = set(jar_signer_sha256(certs.stdout))
    expected = normalized_digest(os.environ.get("SEDS_GS_RELEASE_ANDROID_CERT_SHA256"))
    if certs.returncode or not expected or signers != {expected}:
        raise RuntimeError("The AAB is not signed by exactly the upload keystore certificate")
    pinned = normalized_digest(os.environ.get("ANDROID_EXPECTED_CERT_SHA256"))
    if pinned and pinned != expected:
        raise RuntimeError("The upload certificate does not match ANDROID_EXPECTED_CERT_SHA256")
    android_page_alignment([aab], require_native=True, release=True)
    package = bundletool_value(aab, "/manifest/@package")
    version_code = bundletool_value(aab, "/manifest/@android:versionCode")
    if package != IOS_BUNDLE_ID:
        raise RuntimeError(f"AAB package is {package!r}, expected {IOS_BUNDLE_ID}")
    if version_code != str(info["version_code"]):
        raise RuntimeError(f"AAB versionCode is {version_code!r}, expected {info['version_code']} for {info['tag']}")
    job_summary([f"### Play AAB", f"- Package `{package}`, versionCode {version_code}",
                 f"- Upload certificate SHA-256: `{colon_hex(expected)}`"])


# The oldest iOS SDK App Store Connect takes: 27 (ITMS-90725; a hard error from
# April 2027, a warning before). The TestFlight job builds with Xcode 27.
IOS_STORE_MINIMUM_SDK = 27
# The lowest deployment target App Store Connect takes (ITMS-90068).
IOS_STORE_MINIMUM_OS = (15, 0)


def ios_sdk(output):
    """The SDK major version in `vtool -show-build` output."""
    versions = [int(v) for v in re.findall(r"^\s*sdk (\d+)(?:\.\d+)*\s*$", output, re.MULTILINE)]
    return min(versions, default=None)


def ios_minos(output):
    """The lowest deployment target (major, minor) in `vtool -show-build` output."""
    versions = [(int(major), int(minor or 0))
                for major, minor in re.findall(r"^\s*minos (\d+)(?:\.(\d+))?(?:\.\d+)?\s*$", output, re.MULTILINE)]
    return min(versions, default=None)


def verify_store_ios(info):
    ipa = ROOT / STORE_OUTPUTS["ios"]
    with tempfile.TemporaryDirectory(prefix="ipa-verify-") as temp:
        quiet_run(["ditto", "-x", "-k", str(ipa), temp], "Unpacking the IPA")
        apps = sorted((Path(temp) / "Payload").glob("*.app"))
        if len(apps) != 1:
            raise RuntimeError(f"Expected one app in the IPA Payload, found {len(apps)}")
        app = apps[0]
        plist = plistlib.loads((app / "Info.plist").read_bytes())
        found = (plist.get("CFBundleIdentifier"), plist.get("CFBundleShortVersionString"), plist.get("CFBundleVersion"))
        if found != (IOS_BUNDLE_ID, info["version"], info["build"]):
            raise RuntimeError(f"IPA identity/version {found} != ({IOS_BUNDLE_ID}, {info['version']}, {info['build']})")
        check_appstore_profile(mobileprovision_info(app / "embedded.mobileprovision"))
        verify = subprocess.run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)],
                                capture_output=True, text=True)
        print(verify.stdout + verify.stderr)
        if verify.returncode:
            raise RuntimeError("codesign could not verify the IPA's app")
        executable = app / plistlib.loads((app / "Info.plist").read_bytes())["CFBundleExecutable"]
        build = subprocess.run(["xcrun", "vtool", "-show-build", str(executable)], capture_output=True, text=True).stdout
        sdk = ios_sdk(build)
        if sdk is None or sdk < IOS_STORE_MINIMUM_SDK:
            raise RuntimeError(f"The IPA is built with iOS SDK {sdk}; App Store Connect takes {IOS_STORE_MINIMUM_SDK} or later")
        minimum = ".".join(map(str, IOS_STORE_MINIMUM_OS))
        minos = ios_minos(build)
        if minos is None or minos < IOS_STORE_MINIMUM_OS:
            raise RuntimeError(f"The IPA's executable targets iOS {minos}; App Store Connect takes {minimum} or later")
        declared = str(plist.get("MinimumOSVersion", ""))
        if not re.fullmatch(r"\d+(?:\.\d+){0,2}", declared) or tuple(map(int, declared.split(".")))[:2] < IOS_STORE_MINIMUM_OS:
            raise RuntimeError(f"The IPA declares MinimumOSVersion {declared!r}; App Store Connect takes {minimum} or later")
        details = subprocess.run(["codesign", "-dvv", str(app)], capture_output=True, text=True)
        authority = re.search(r"^Authority=((?:Apple|iPhone) Distribution: .+)$", details.stdout + details.stderr,
                              re.MULTILINE)
        if details.returncode or not authority:
            raise RuntimeError("The IPA's app is not signed with an Apple Distribution identity")
    job_summary([f"### TestFlight IPA", f"- `{IOS_BUNDLE_ID}` {info['version']} ({info['build']})",
                 f"- Signed by `{authority.group(1)}`, App Store profile embedded"])


def store_verify(name, tag):
    info = release_version(tag)
    if name == "android":
        verify_store_android(info)
    elif name == "ios":
        verify_store_ios(info)
    else:
        raise RuntimeError(f"{name} is not a store target")


def appstore_key_dir():
    """altool looks for AuthKey_<id>.p8 under ~/.appstoreconnect/private_keys
    (or API_PRIVATE_KEYS_DIR); keep that home inside the signing directory."""
    home = release_signing_dir() / "appstoreconnect-home"
    return home, home / ".appstoreconnect" / "private_keys"


def upload_testflight(info):
    require_secrets(NOTARY_API_SECRETS, "TestFlight upload")
    key_id = os.environ["APPLE_NOTARY_KEY_ID"].strip()
    issuer = os.environ["APPLE_NOTARY_ISSUER_ID"].strip()
    if not re.fullmatch(r"[A-Z0-9]{6,20}", key_id):
        raise RuntimeError("APPLE_NOTARY_KEY_ID does not look like an App Store Connect key ID")
    print(f"::add-mask::{issuer}", flush=True)
    home, keys = appstore_key_dir()
    keys.mkdir(mode=0o700, parents=True)
    for directory in (home, home / ".appstoreconnect", keys):
        directory.chmod(0o700)
    key = keys / f"AuthKey_{key_id}.p8"
    private_write(key, decode_secret("APPLE_NOTARY_KEY_P8_B64"))
    env = dict(os.environ, HOME=str(home), API_PRIVATE_KEYS_DIR=str(keys))
    for name in [*IOS_STORE_SECRETS, *NOTARY_API_SECRETS]:
        env.pop(name, None)
    ipa = ROOT / STORE_OUTPUTS["ios"]
    try:
        result = subprocess.run(["xcrun", "altool", "--upload-app", "--type", "ios", "--file", str(ipa),
                                 "--apiKey", key_id, "--apiIssuer", issuer, "--output-format", "json"],
                                cwd=home, env=env, capture_output=True, text=True)
    finally:
        key.unlink(missing_ok=True)
    output = (result.stdout + result.stderr).replace(issuer, "***")
    print(output)
    if result.returncode:
        if re.search(r"Redundant Binary Upload|already (?:been )?uploaded a build", output, re.IGNORECASE):
            job_summary([f"### TestFlight", f"Build {info['build']} of {info['version']} was already uploaded; nothing to do."])
            return "already-uploaded"
        raise RuntimeError(f"altool upload failed (exit {result.returncode})")
    job_summary([f"### TestFlight", f"Uploaded {info['name']} to App Store Connect. After processing it is "
                 "available to TestFlight testers; it is not submitted for App Store review."])
    return "uploaded"


# App Store Connect API: TestFlight beta groups. Uses the same API key as the
# altool upload (APPLE_NOTARY_KEY_*), as an ES256 JWT signed by openssl.
ASC_API = "https://api.appstoreconnect.apple.com"
ASC_AUDIENCE = "appstoreconnect-v1"
ASC_TOKEN_LIFETIME = 20 * 60          # Apple's maximum
ASC_TOKEN_REFRESH = 15 * 60
ASC_PROCESSING_TIMEOUT = 60 * 60
ASC_POLL_INTERVAL = 60
TESTFLIGHT_NOTES_LIMIT = 4000
NOTES_LOCALE = "en-US"
# Every App Store Connect call this driver can make. Nothing that submits for
# App Store review (appStoreVersionSubmissions, reviewSubmissions) or touches
# an App Store version is reachable; betaAppReviewSubmissions is TestFlight's
# beta review for external groups.
ASC_ALLOWED = (
    ("GET", r"/v1/apps"),
    ("GET", r"/v1/builds"),
    ("GET", r"/v1/betaGroups"),
    ("POST", r"/v1/builds/[A-Za-z0-9-]+/relationships/betaGroups"),
    ("GET", r"/v1/betaBuildLocalizations"),
    ("POST", r"/v1/betaBuildLocalizations"),
    ("PATCH", r"/v1/betaBuildLocalizations/[A-Za-z0-9-]+"),
    ("GET", r"/v1/betaAppReviewSubmissions"),
    ("POST", r"/v1/betaAppReviewSubmissions"),
)


def asc_allowed(method, path):
    if not any(method == m and re.fullmatch(pattern, path) for m, pattern in ASC_ALLOWED):
        raise RuntimeError(f"Refusing App Store Connect call {method} {path}: not a TestFlight beta-group call")


def ensure_release_signing_dir():
    directory = release_signing_dir()
    return directory if directory.is_dir() else release_signing_dir(create=True)


def release_notes(tag, limit):
    """docs/release-notes/<tag>.md as plain text for a store, cut to limit."""
    if not RELEASE_TAG.fullmatch(tag or ""):
        return None
    path = ROOT / "docs" / "release-notes" / f"{tag}.md"
    if not path.is_file():
        return None
    text = path.read_text(encoding="utf-8").strip()
    if len(text) > limit:
        text = text[:limit - 1].rstrip() + "…"
    return text or None


def der_ecdsa_to_raw(der, size=32):
    """openssl writes an ECDSA signature as DER SEQUENCE{r, s}; a JWS ES256
    signature is r || s, each 32 bytes big-endian."""
    def length(i):
        first = der[i]
        if first < 0x80:
            return first, i + 1
        count = first & 0x7F
        return int.from_bytes(der[i + 1:i + 1 + count], "big"), i + 1 + count
    try:
        if der[0] != 0x30:
            raise ValueError
        _total, i = length(1)
        raw = b""
        for _ in range(2):
            if der[i] != 0x02:
                raise ValueError
            count, i = length(i + 1)
            raw += int.from_bytes(der[i:i + count], "big").to_bytes(size, "big")
            i += count
        return raw
    except (IndexError, ValueError, OverflowError):
        raise RuntimeError("openssl returned a malformed ECDSA signature") from None


def asc_credentials():
    require_secrets(NOTARY_API_SECRETS, "App Store Connect API")
    key_id = os.environ["APPLE_NOTARY_KEY_ID"].strip()
    issuer = os.environ["APPLE_NOTARY_ISSUER_ID"].strip()
    if not re.fullmatch(r"[A-Z0-9]{6,20}", key_id):
        raise RuntimeError("APPLE_NOTARY_KEY_ID does not look like an App Store Connect key ID")
    print(f"::add-mask::{issuer}", flush=True)
    return key_id, issuer


def asc_token(key_id, issuer, now=None):
    """ES256 App Store Connect JWT. The .p8 exists on disk only while openssl
    signs; the token is masked in the log."""
    now = int(time.time() if now is None else now)
    header = b64url(json.dumps({"alg": "ES256", "kid": key_id, "typ": "JWT"}).encode())
    claims = b64url(json.dumps({"iss": issuer, "iat": now, "exp": now + ASC_TOKEN_LIFETIME,
                                "aud": ASC_AUDIENCE}).encode())
    unsigned = f"{header}.{claims}".encode()
    key_path = ensure_release_signing_dir() / f"asc-{secrets.token_hex(8)}.p8"
    private_write(key_path, decode_secret("APPLE_NOTARY_KEY_P8_B64"))
    try:
        signature = subprocess.run(["openssl", "dgst", "-sha256", "-sign", str(key_path)], input=unsigned,
                                   capture_output=True)
    finally:
        key_path.unlink(missing_ok=True)
    if signature.returncode or not signature.stdout:
        raise RuntimeError("Could not sign the App Store Connect token with APPLE_NOTARY_KEY_P8_B64")
    token = unsigned.decode() + "." + b64url(der_ecdsa_to_raw(signature.stdout))
    print(f"::add-mask::{token}", flush=True)
    return token


class AscError(RuntimeError):
    def __init__(self, status, message):
        super().__init__(f"App Store Connect API HTTP {status}: {message}")
        self.status, self.message = status, message


def asc_error_message(raw):
    try:
        errors = json.loads(raw).get("errors") or []
        return "; ".join(f"{e.get('code')}: {e.get('detail') or e.get('title')}" for e in errors) or "no detail"
    except (ValueError, AttributeError):
        return raw[:500].decode("utf-8", errors="replace") if isinstance(raw, bytes) else str(raw)[:500]


class AppStoreConnect:
    """call(method, path, body=None, query=None) against the App Store Connect
    API; only ASC_ALLOWED calls; a fresh token before each one expires."""

    def __init__(self, key_id, issuer, clock=time.time):
        self.key_id, self.issuer, self.clock = key_id, issuer, clock
        self.token, self.issued = None, 0

    def __call__(self, method, path, body=None, query=None):
        asc_allowed(method, path)
        now = self.clock()
        if self.token is None or now - self.issued > ASC_TOKEN_REFRESH:
            self.token, self.issued = asc_token(self.key_id, self.issuer, now), now
        url = ASC_API + path + ("?" + urllib.parse.urlencode(query) if query else "")
        headers = {"Authorization": f"Bearer {self.token}"}
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(url, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                raw = response.read()
        except urllib.error.HTTPError as error:
            raise AscError(error.code, asc_error_message(error.read())) from None
        return json.loads(raw) if raw.strip() else {}


def asc_wait_for_build(info, app_id, call, *, timeout, interval, sleep, clock):
    """The App Store Connect build for this version and build number once it
    has processed (VALID), or None when timeout passes first."""
    query = {"filter[app]": app_id, "filter[version]": info["build"],
             "filter[preReleaseVersion.version]": info["version"],
             "filter[preReleaseVersion.platform]": "IOS", "limit": "10"}
    deadline = clock() + timeout
    while True:
        builds = call("GET", "/v1/builds", query=query).get("data") or []
        if len(builds) > 1:
            raise RuntimeError(f"App Store Connect lists {len(builds)} builds for {info['version']} "
                               f"({info['build']})")
        if builds:
            state = (builds[0].get("attributes") or {}).get("processingState")
            if state == "VALID":
                return builds[0]
            if state in {"FAILED", "INVALID"}:
                raise RuntimeError(f"App Store Connect processing of {info['version']} ({info['build']}) "
                                   f"ended {state}; see the email from App Store Connect")
        remaining = deadline - clock()
        if remaining <= 0:
            return None
        sleep(min(interval, remaining))


def set_what_to_test(build_id, text, call, locale=NOTES_LOCALE):
    found = call("GET", "/v1/betaBuildLocalizations", query={"filter[build]": build_id, "limit": "50"})
    existing = [d for d in found.get("data") or [] if (d.get("attributes") or {}).get("locale") == locale]
    if existing:
        call("PATCH", f"/v1/betaBuildLocalizations/{existing[0]['id']}",
             body={"data": {"type": "betaBuildLocalizations", "id": existing[0]["id"],
                            "attributes": {"whatsNew": text}}})
    else:
        call("POST", "/v1/betaBuildLocalizations",
             body={"data": {"type": "betaBuildLocalizations", "attributes": {"locale": locale, "whatsNew": text},
                            "relationships": {"build": {"data": {"type": "builds", "id": build_id}}}}})


def testflight_distribute(info, groups, call, *, timeout=ASC_PROCESSING_TIMEOUT, interval=ASC_POLL_INTERVAL,
                          sleep=time.sleep, clock=time.monotonic):
    """Add this release's TestFlight build to the named beta groups. Waits for
    processing (a timeout only warns). If a group is external, sets What to
    Test from the release notes and submits the build for *beta* app review.
    Never submits anything for App Store review."""
    if not groups:
        return "no-groups"
    apps = call("GET", "/v1/apps", query={"filter[bundleId]": IOS_BUNDLE_ID}).get("data") or []
    app = next((a["id"] for a in apps if (a.get("attributes") or {}).get("bundleId") == IOS_BUNDLE_ID), None)
    if not app:
        raise RuntimeError(f"App Store Connect has no app with bundle id {IOS_BUNDLE_ID} for this API key")
    build = asc_wait_for_build(info, app, call, timeout=timeout, interval=interval, sleep=sleep, clock=clock)
    if build is None:
        message = (f"{info['version']} ({info['build']}) was still processing after {timeout // 60} min, so it "
                   f"was not added to {', '.join(groups)}. Run promote.yml with this tag and the groups once "
                   "it has processed.")
        print(f"::warning title=TestFlight processing::{message}", flush=True)
        job_summary(["### TestFlight groups", message])
        return "processing-timeout"
    build_id = build["id"]
    listed = call("GET", "/v1/betaGroups", query={"filter[app]": app, "limit": "200"}).get("data") or []
    by_name = {(g.get("attributes") or {}).get("name"): g for g in listed}
    missing = [name for name in groups if name not in by_name]
    if missing:
        raise RuntimeError(f"No TestFlight beta group named {', '.join(missing)}; the app has "
                           f"{', '.join(sorted(n for n in by_name if n)) or 'no groups'}")
    chosen = [by_name[name] for name in groups]
    external = [name for name, group in zip(groups, chosen) if not group["attributes"].get("isInternalGroup")]
    notes = release_notes(info["tag"], TESTFLIGHT_NOTES_LIMIT)
    if notes:
        set_what_to_test(build_id, notes, call)
    elif external:
        print("::warning title=TestFlight What to Test::No docs/release-notes/"
              f"{info['tag']}.md, so What to Test is not set; beta app review for external groups needs it",
              flush=True)
    if external and (build.get("attributes") or {}).get("usesNonExemptEncryption") is None:
        print("::warning title=TestFlight export compliance::The build has no encryption answer "
              "(ITSAppUsesNonExemptEncryption); answer it in App Store Connect or beta review cannot start",
              flush=True)
    call("POST", f"/v1/builds/{build_id}/relationships/betaGroups",
         body={"data": [{"type": "betaGroups", "id": group["id"]} for group in chosen]})
    review = ""
    if external:
        submitted = call("GET", "/v1/betaAppReviewSubmissions", query={"filter[build]": build_id}).get("data") or []
        if submitted:
            state = (submitted[0].get("attributes") or {}).get("betaReviewState") or "submitted"
            review = f" Beta app review was already requested ({state})."
        else:
            call("POST", "/v1/betaAppReviewSubmissions",
                 body={"data": {"type": "betaAppReviewSubmissions",
                                "relationships": {"build": {"data": {"type": "builds", "id": build_id}}}}})
            review = " Submitted for beta app review (external groups); testers get it once Apple approves."
    job_summary(["### TestFlight groups",
                 f"Added {info['version']} ({info['build']}) to {', '.join(groups)}"
                 + (f" (external: {', '.join(external)})." if external else " (internal only).")
                 + review + (" What to Test set from the release notes." if notes else "")
                 + " Not submitted for App Store review."])
    return "distributed"


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def google_access_token(account, key_path):
    """OAuth2 service-account JWT bearer grant, signed by openssl (no extra
    Python packages). The token is masked in the log."""
    token_uri = account.get("token_uri") or GOOGLE_TOKEN_URI
    if token_uri != GOOGLE_TOKEN_URI:
        raise RuntimeError("PLAY_SERVICE_ACCOUNT_JSON_B64 names an unexpected token_uri")
    now = int(time.time())
    header = b64url(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
    claims = b64url(json.dumps({"iss": account["client_email"], "scope": PLAY_SCOPE, "aud": token_uri,
                                "iat": now, "exp": now + 3600}).encode())
    unsigned = f"{header}.{claims}".encode()
    signature = subprocess.run(["openssl", "dgst", "-sha256", "-sign", str(key_path)], input=unsigned,
                               capture_output=True)
    if signature.returncode or not signature.stdout:
        raise RuntimeError("Could not sign the Google service-account assertion with its private key")
    body = urllib.parse.urlencode({"grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
                                   "assertion": unsigned.decode() + "." + b64url(signature.stdout)}).encode()
    request = urllib.request.Request(token_uri, data=body, method="POST",
                                     headers={"Content-Type": "application/x-www-form-urlencoded"})
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            token = json.loads(response.read())["access_token"]
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"Google OAuth token request failed: HTTP {error.code} "
                           f"{play_error_message(error.read())}") from None
    print(f"::add-mask::{token}", flush=True)
    return token


def play_error_message(raw):
    try:
        error = json.loads(raw).get("error")
    except (ValueError, AttributeError):
        return raw[:500].decode("utf-8", errors="replace") if isinstance(raw, bytes) else str(raw)[:500]
    if isinstance(error, dict):
        return str(error.get("message") or error.get("status") or error)
    return str(error)


class PlayError(RuntimeError):
    def __init__(self, status, message):
        super().__init__(f"Google Play API HTTP {status}: {message}")
        self.status, self.message = status, message


def play_call(token, method, url, *, body=None, data=None, content_type="application/json", timeout=120):
    headers = {"Authorization": f"Bearer {token}"}
    if body is not None:
        data = json.dumps(body).encode()
    if data is not None:
        headers["Content-Type"] = content_type
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            raw = response.read()
    except urllib.error.HTTPError as error:
        raise PlayError(error.code, play_error_message(error.read())) from None
    return json.loads(raw) if raw.strip() else {}


def play_release(version_code, name, status, track=PLAY_TRACK, notes=None):
    if track not in PLAY_TRACKS:
        raise RuntimeError(f"Refusing Play track {track!r}")
    release = {"name": name, "versionCodes": [str(version_code)], "status": status}
    if notes:
        release["releaseNotes"] = [{"language": NOTES_LOCALE, "text": notes}]
    return {"track": track, "releases": [release]}


def play_service_token():
    try:
        account = json.loads(decode_secret("PLAY_SERVICE_ACCOUNT_JSON_B64"))
        private_key = account["private_key"].encode()
        account["client_email"]
    except (ValueError, KeyError, TypeError, AttributeError):
        raise RuntimeError("PLAY_SERVICE_ACCOUNT_JSON_B64 is not a base64 service-account JSON key") from None
    key_path = ensure_release_signing_dir() / "play-service-account.pem"
    private_write(key_path, private_key)
    try:
        return google_access_token(account, key_path)
    finally:
        key_path.unlink(missing_ok=True)


def play_codes(track_body):
    return {str(code) for release in track_body.get("releases", []) for code in release.get("versionCodes", [])}


def play_set_track(call, token, edit_url, track, info, notes):
    """PUT exactly this versionCode on track and commit the edit. Status is
    completed, or draft while Play still treats the app as a draft app."""
    status = "completed"
    for attempt in range(2):
        call(token, "PUT", f"{edit_url}/tracks/{track}",
             body=play_release(info["version_code"], info["name"], status, track, notes))
        try:
            call(token, "POST", f"{edit_url}:commit")
            return status
        except PlayError as error:
            # An app that has never been published accepts only draft releases.
            if attempt or "draft" not in error.message.lower():
                raise
            status = "draft"


def upload_play(info, call=play_call, track=None):
    """One edit: upload the AAB, put exactly its versionCode on the selected
    testing track (PLAY_TRACK, default internal), commit. A versionCode Play
    has already seen is not re-uploaded."""
    track = play_track(track)
    require_secrets(PLAY_STORE_SECRETS, "Google Play testing-track upload")
    token = play_service_token()
    package = IOS_BUNDLE_ID
    aab = ROOT / STORE_OUTPUTS["android"]
    base = f"{PLAY_API}/{package}/edits"
    notes = release_notes(info["tag"], PLAY_NOTES_LIMIT)
    edit = call(token, "POST", base, body={})["id"]
    committed = False
    try:
        try:
            uploaded = call(token, "POST", f"{PLAY_UPLOAD_API}/{package}/edits/{edit}/bundles?uploadType=media",
                            data=aab.read_bytes(), content_type="application/octet-stream", timeout=1800)
        except PlayError as error:
            if not re.search(r"version code .*already been used|apkUpgradeVersionConflict", error.message, re.IGNORECASE):
                raise
            if str(info["version_code"]) not in play_codes(call(token, "GET", f"{base}/{edit}/tracks/{track}")):
                raise RuntimeError(f"versionCode {info['version_code']} was already used on Play but is not on the "
                                   f"{track} track; promote it with promote.yml, or bump Dioxus.toml build") from None
            job_summary(["### Google Play", f"versionCode {info['version_code']} is already on the {track} track."])
            return "already-uploaded"
        if str(uploaded.get("versionCode")) != str(info["version_code"]):
            raise RuntimeError(f"Play reports versionCode {uploaded.get('versionCode')}, expected {info['version_code']}")
        status = play_set_track(call, token, f"{base}/{edit}", track, info, notes)
        committed = True
    finally:
        if not committed:
            try:
                call(token, "DELETE", f"{base}/{edit}")
            except PlayError:
                pass
    job_summary(["### Google Play", f"Uploaded versionCode {info['version_code']} ({info['name']}) to the "
                 f"**{track}** testing track as a {status} release"
                 + (" with release notes." if notes else ".")
                 + (" Roll it out from Play Console (the app has never been published)." if status == "draft" else "")])
    return "uploaded"


def promote_play(info, from_track, to_track, call=play_call):
    """Put the versionCode already on from_track on a later testing track
    (internal -> alpha -> beta). Never production; the source track is kept."""
    source, target = play_track(from_track), play_track(to_track)
    if PLAY_TRACKS.index(source) >= PLAY_TRACKS.index(target):
        raise RuntimeError(f"Promotion goes {' -> '.join(PLAY_TRACKS)}; {source} -> {target} is not a promotion")
    require_secrets(PLAY_STORE_SECRETS, "Google Play track promotion")
    token = play_service_token()
    base = f"{PLAY_API}/{IOS_BUNDLE_ID}/edits"
    code = str(info["version_code"])
    edit = call(token, "POST", base, body={})["id"]
    committed = False
    try:
        if code not in play_codes(call(token, "GET", f"{base}/{edit}/tracks/{source}")):
            raise RuntimeError(f"versionCode {code} ({info['tag']}) is not on the {source} track")
        if code in play_codes(call(token, "GET", f"{base}/{edit}/tracks/{target}")):
            job_summary(["### Google Play", f"versionCode {code} is already on the {target} track."])
            return "already-promoted"
        status = play_set_track(call, token, f"{base}/{edit}", target, info,
                                release_notes(info["tag"], PLAY_NOTES_LIMIT))
        committed = True
    finally:
        if not committed:
            try:
                call(token, "DELETE", f"{base}/{edit}")
            except PlayError:
                pass
    job_summary(["### Google Play", f"Promoted versionCode {code} ({info['name']}) from {source} to "
                 f"**{target}** as a {status} release. Production is never touched."])
    return "promoted"


def store_upload(name, tag):
    info = release_version(tag)
    if name == "ios":
        groups = testflight_groups()
        outcome = upload_testflight(info)
        if groups:
            testflight_distribute(info, groups, AppStoreConnect(*asc_credentials()))
        return outcome
    if name == "android":
        return upload_play(info)
    raise RuntimeError(f"{name} is not a store target")


def store_promote(name, tag):
    """promote.yml: move an existing release between testing tracks/groups."""
    info = release_version(tag)
    if name == "ios":
        groups = testflight_groups()
        if not groups:
            raise RuntimeError("TESTFLIGHT_GROUPS names no beta group to add the build to")
        return testflight_distribute(info, groups, AppStoreConnect(*asc_credentials()), timeout=10 * 60)
    if name == "android":
        return promote_play(info, os.environ.get("PLAY_PROMOTE_FROM", ""), os.environ.get("PLAY_TRACK", ""))
    raise RuntimeError(f"{name} is not a store target")


def main():
    configure_utf8_console()
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["bootstrap", "refetch", "build", "release-version", "release-guard",
                                           "release-android-signing", "release-macos-signing", "release-ios-signing",
                                           "release-build", "release-verify", "release-stage", "release-checksums",
                                           "release-store-check", "release-store-build", "release-store-verify",
                                           "release-store-upload", "release-store-promote", "release-cleanup"])
    parser.add_argument("--target", choices=[p["target"] for p in PLATFORMS], default=os.environ.get("CI_FRONTEND_TARGET"))
    parser.add_argument("--profile", choices=["debug", "release"], default=os.environ.get("CI_FRONTEND_PROFILE", "release"))
    parser.add_argument("--tag", help="release-version: the pushed tag, e.g. v0.3.1b36")
    parser.add_argument("--assets", help="release-checksums: directory holding every downloaded asset")
    parser.add_argument("--release-targets", help="release-checksums: comma-separated targets that were built")
    parser.add_argument("--store", choices=sorted(STORES), help="release-store-check: the store (or promotion) to check")
    args = parser.parse_args()
    if args.action in {"bootstrap", "build", "release-build", "release-verify", "release-stage",
                       "release-store-build", "release-store-verify", "release-store-upload",
                       "release-store-promote"} and not args.target:
        parser.error(f"{args.action} needs --target or CI_FRONTEND_TARGET")
    if args.action in {"release-guard", "release-store-verify", "release-store-upload", "release-store-promote"}:
        args.tag = args.tag or os.environ.get("RELEASE_TAG")
        if not args.tag:
            parser.error(f"{args.action} needs --tag or RELEASE_TAG")
    if args.action == "bootstrap":
        bootstrap(args.target)
    elif args.action == "refetch":
        # The dependency cache restores Cargo's registry index as it was when
        # the cache was saved, which can be older than the lock bootstrap just
        # resolved; the build then runs offline and cannot find newer crates.
        # Fetch the current lock again over the restored registry.
        fetch_application(ci_env(), "application-refetch")
    elif args.action == "build":
        run(args.target, args.profile)
    elif args.action == "release-version":
        info = release_version(args.tag)
        github_file("GITHUB_OUTPUT", info)
        job_summary([f"## {info['name']}", f"Tag `{info['tag']}`, Android versionCode {info['version_code']}."])
    elif args.action == "release-guard":
        result = release_guard(args.tag)
        github_file("GITHUB_OUTPUT", result)
        job_summary([f"Release tag `{args.tag}` is commit `{result['commit']}` on `origin/{result['release_branch']}`, "
                     "from a protected ref."])
    elif args.action == "release-store-check":
        if not args.store:
            parser.error("release-store-check needs --store")
        store_check(args.store)
    elif args.action == "release-ios-signing":
        prepare_ios_release_signing()
    elif args.action == "release-store-build":
        store_build(args.target)
    elif args.action == "release-store-verify":
        store_verify(args.target, args.tag)
    elif args.action == "release-store-upload":
        store_upload(args.target, args.tag)
    elif args.action == "release-store-promote":
        store_promote(args.target, args.tag)
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

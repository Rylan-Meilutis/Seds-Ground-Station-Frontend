"""Explicit frontend targets and isolated Docker builds. No flashing or deployment."""
import argparse
from dataclasses import dataclass
import os
from pathlib import Path
import platform
import plistlib
import shlex
import shutil
import subprocess
import sys

@dataclass(frozen=True)
class Target:
    platform: str
    # Rust triple of the phone/desktop app, or None for a target without Rust
    # (the Wear OS companion is Kotlin only).
    triple: str | None
    simulator: bool = False
    docker: str | None = None
    container_arch: str = "amd64"
    # Watch companion built by this target: "watchos" (Swift app from
    # watch/apple, embedded in the iOS app) or "wearos" (watch/wear APK).
    watch: str | None = None
    # Apple ARCHS for the watchOS app, or the Wear OS ABIs the target is for.
    # The Wear app has no native code, so its APK runs on every listed ABI.
    watch_archs: tuple[str, ...] = ()

TARGETS = {
    "web": Target("web", "wasm32-unknown-unknown", docker="base"),
    "linux": Target("linux", "x86_64-unknown-linux-gnu", docker="base"),
    "linux-arm64": Target("linux", "aarch64-unknown-linux-gnu", docker="base", container_arch="arm64"),
    "windows": Target("windows", "x86_64-pc-windows-msvc"),
    "macos": Target("macos", "aarch64-apple-darwin"),
    "macos-intel": Target("macos", "x86_64-apple-darwin"),
    "ios": Target("ios", "aarch64-apple-ios"),
    "android": Target("android", "aarch64-linux-android", docker="android"),
    "android-armv7": Target("android", "armv7-linux-androideabi", docker="android"),
    # iOS app (aarch64-apple-ios) with the Apple Watch app embedded at
    # GS26.app/Watch/GS26Watch.app; arm64_32 (Series 4-8, SE, Ultra 1) and
    # arm64 (Series 9+, Ultra 2+) watch slices.
    "watchos": Target("ios", "aarch64-apple-ios", watch="watchos", watch_archs=("arm64_32", "arm64")),
    "wearos": Target("wearos", None, docker="android", watch="wearos", watch_archs=("arm64-v8a", "armeabi-v7a")),
    "ios_sim": Target("ios", "aarch64-apple-ios-sim", simulator=True),
    "ios_sim_intel": Target("ios", "x86_64-apple-ios", simulator=True),
    "android_sim": Target("android", "x86_64-linux-android", simulator=True, docker="android"),
    "watchos_sim": Target("ios", "aarch64-apple-ios-sim", simulator=True, watch="watchos", watch_archs=("arm64",)),
    "watchos_sim_intel": Target("ios", "x86_64-apple-ios", simulator=True, watch="watchos", watch_archs=("x86_64",)),
    "wearos_sim": Target("wearos", None, simulator=True, docker="android", watch="wearos", watch_archs=("x86_64",)),
}

WATCH_APPLE = Path("watch") / "apple"
WATCH_WEAR = Path("watch") / "wear"
WATCH_APP_NAME = "GS26Watch.app"
WEAR_APK_NAME = "GS26-WearOS.apk"


XCODE_INSTALL_HINT = ("install Xcode from the App Store, open it once, then run\n"
                      "    sudo xcode-select -s /Applications/Xcode.app/Contents/Developer")
ANDROID_SDK_HINT = ("install Android Studio (`brew install --cask android-studio`) or the SDK command-line\n"
                    "    tools, then set ANDROID_HOME to the SDK (usually ~/Library/Android/sdk or ~/Android/Sdk)")
ADB_HINT = "install the SDK platform-tools (`sdkmanager platform-tools`, or `brew install --cask android-platform-tools`)"


def xcode_selected():
    """True when xcodebuild runs, i.e. the full Xcode is selected and not only
    the Command Line Tools (whose /usr/bin/xcodebuild shim exits with an error)."""
    try:
        subprocess.run(["xcodebuild", "-version"], capture_output=True, check=True, timeout=120)
        return True
    except (OSError, subprocess.SubprocessError):
        return False


def apple_watch_tool_problem(*, xcodegen=True, ios_deploy=False, which=shutil.which, xcode_ok=xcode_selected):
    """Missing tools for the Apple Watch build/deploy, with the install
    command, or None. Checked before any build work."""
    brew = [tool for tool, needed in (("xcodegen", xcodegen), ("ios-deploy", ios_deploy))
            if needed and not which(tool)]
    problems = []
    if brew:
        problems.append(f"{' and '.join(brew)} not found: brew install {' '.join(brew)}")
    absent = [tool for tool in ("xcrun", "xcodebuild") if not which(tool)]
    if absent:
        problems.append(f"{' and '.join(absent)} not found: {XCODE_INSTALL_HINT}")
    elif not xcode_ok():
        problems.append(f"`xcodebuild -version` failed (only the Command Line Tools are selected?): {XCODE_INSTALL_HINT}")
    return "\n  ".join(problems) or None


def android_sdk_root(environ):
    for key in ("ANDROID_HOME", "ANDROID_SDK_ROOT"):
        value = (environ.get(key) or "").strip()
        if value:
            return Path(value).expanduser()
    return None


def wear_tool_problem(sdk_root, *, adb=False, path=None, which=shutil.which):
    """Missing Android SDK (and, for deploys, adb) with how to install it, or None."""
    if sdk_root is None or not Path(sdk_root).is_dir():
        where = f" ({sdk_root} does not exist)" if sdk_root else ""
        return f"Android SDK not found{where}: {ANDROID_SDK_HINT}"
    if adb and not (which("adb", path=path) or (Path(sdk_root) / "platform-tools" / "adb").is_file()):
        return f"adb not found: {ADB_HINT}"
    return None


def watch_tool_problem(name, environ=None):
    """Preflight for the watch build targets (no deploy tools)."""
    target = TARGETS[name]
    if target.watch == "watchos":
        return apple_watch_tool_problem()
    if target.watch == "wearos":
        return wear_tool_problem(android_sdk_root(os.environ if environ is None else environ))
    return None


def watchos_build_commands(root, name, debug):
    """Generate the Xcode project and build the watch app unsigned.

    Device builds are signed later together with the iOS app (inner app
    first, see scripts/ios_package_sign.sh); simulator builds need no
    identity."""
    target = TARGETS[name]
    apple = root / WATCH_APPLE
    sdk = "watchsimulator" if target.simulator else "watchos"
    destination = "generic/platform=watchOS Simulator" if target.simulator else "generic/platform=watchOS"
    return [
        ["xcodegen", "generate", "--spec", str(apple / "project.yml"), "--project", str(apple)],
        ["xcodebuild", "-project", str(apple / "GS26Watch.xcodeproj"), "-scheme", "GS26Watch",
         "-configuration", "Debug" if debug else "Release", "-sdk", sdk, "-destination", destination,
         "-derivedDataPath", str(root / "target" / "watch" / name),
         "ARCHS=" + " ".join(target.watch_archs), "ONLY_ACTIVE_ARCH=NO",
         "CODE_SIGNING_ALLOWED=NO", "build"],
    ]


def watchos_product(root, name, debug):
    target = TARGETS[name]
    sdk = "watchsimulator" if target.simulator else "watchos"
    config = "Debug" if debug else "Release"
    return root / "target" / "watch" / name / "Build" / "Products" / f"{config}-{sdk}" / WATCH_APP_NAME


def staged_ios_app(root):
    apps = sorted((root / "dist").glob("*.app"))
    if len(apps) != 1:
        raise RuntimeError(f"Expected exactly one staged iOS app in dist/, found {len(apps)}")
    return apps[0]


def embed_watch_app(watch_app, ios_app):
    """Copy the watch app into the iOS app as Watch/<name>.app after checking
    that it names this iOS app as its companion."""
    watch_app, ios_app = Path(watch_app), Path(ios_app)
    with (watch_app / "Info.plist").open("rb") as f: watch_info = plistlib.load(f)
    with (ios_app / "Info.plist").open("rb") as f: ios_info = plistlib.load(f)
    phone_id = ios_info.get("CFBundleIdentifier")
    if watch_info.get("WKCompanionAppBundleIdentifier") != phone_id:
        raise RuntimeError(f"Watch app companion id {watch_info.get('WKCompanionAppBundleIdentifier')!r} does not match iOS app {phone_id!r}")
    if not str(watch_info.get("CFBundleIdentifier", "")).startswith(f"{phone_id}."):
        raise RuntimeError(f"Watch app bundle id must be prefixed by {phone_id!r}")
    destination = ios_app / "Watch" / watch_app.name
    if destination.exists(): shutil.rmtree(destination)
    destination.parent.mkdir(exist_ok=True)
    shutil.copytree(watch_app, destination, symlinks=True)
    return destination


def simulator_adhoc_sign_commands(ios_app, embedded):
    # Embedding changed the iOS app's contents; re-seal both ad hoc.
    return [["codesign", "--force", "--sign", "-", "--timestamp=none", str(path)] for path in (embedded, ios_app)]


def wear_application_id(root):
    import tomllib
    try: bundle = tomllib.loads((root / "Dioxus.toml").read_text(encoding="utf-8")).get("bundle", {})
    except (OSError, ValueError): bundle = {}
    return bundle.get("identifier") or "com.UBSEDS.GS26"


def wearos_signing_properties(environ):
    """Release signing for the Wear APK. The Data Layer pairs only apps with
    the phone's signing key: CI uses its job-local development key, local
    builds the same ANDROID_* upload key settings as the phone app."""
    store = environ.get("SEDS_GS_CI_ANDROID_KEYSTORE", "")
    if store:
        password_file = environ.get("SEDS_GS_CI_ANDROID_KEYSTORE_PASSWORD_FILE", "")
        password = Path(password_file).read_text(encoding="utf-8").strip() if password_file else ""
        alias = "androiddebugkey"
        key_password = password
    else:
        store = environ.get("ANDROID_KEYSTORE_PATH", "")
        alias = environ.get("ANDROID_KEY_ALIAS", "")
        password = environ.get("ANDROID_KEYSTORE_PASSWORD", "")
        key_password = environ.get("ANDROID_KEY_PASSWORD", "") or password
    if not (store and alias and password):
        return {}
    # Passed as ORG_GRADLE_PROJECT_* environment, so no password is on argv.
    return {"ORG_GRADLE_PROJECT_android.injected.signing.store.file": str(Path(store).expanduser()),
            "ORG_GRADLE_PROJECT_android.injected.signing.store.password": password,
            "ORG_GRADLE_PROJECT_android.injected.signing.key.alias": alias,
            "ORG_GRADLE_PROJECT_android.injected.signing.key.password": key_password}


def wearos_build_command(root, name, debug):
    """Gradle build of the Wear OS companion: core unit tests, then the APK.
    Release signing comes from wearos_signing_properties (environment)."""
    wear = root / WATCH_WEAR
    gradlew = wear / ("gradlew.bat" if platform.system() == "Windows" else "gradlew")
    task = ":app:assembleDebug" if debug else ":app:assembleRelease"
    return [str(gradlew), "-p", str(wear), "--no-daemon", "--console=plain",
            f"-Pgs26.applicationId={wear_application_id(root)}", ":core:test", task]


def wearos_product(root, debug):
    outputs = root / WATCH_WEAR / "app" / "build" / "outputs" / "apk"
    if debug: return outputs / "debug" / "app-debug.apk"
    signed = outputs / "release" / "app-release.apk"
    return signed if signed.exists() else outputs / "release" / "app-release-unsigned.apk"


def build_watch(root, name, args, *, package_ios=None):
    """Build the watch companion for a watch target after its phone app (if
    any) was built into dist/. Returns the watch artifact path."""
    target = TARGETS[name]
    run = subprocess.run
    if target.watch == "wearos":
        command = wearos_build_command(root, name, args.debug)
        # Like the phone APK: the CI development key always signs; --no-sign
        # only skips the local upload key.
        environ = {k: v for k, v in os.environ.items() if k.startswith("SEDS_GS_CI_ANDROID_") or not args.no_sign}
        signing = {} if args.debug else wearos_signing_properties(environ)
        print(shlex.join(command) + (" (release signing from ANDROID_* / CI key)" if signing else ""), flush=True)
        if args.dry_run: return None
        if signing: run(command, cwd=root, check=True, env={**os.environ, **signing})
        else: run(command, cwd=root, check=True)
        apk = wearos_product(root, args.debug)
        if not apk.is_file(): raise RuntimeError(f"Wear OS build produced no APK: {apk}")
        output = root / "dist" / WEAR_APK_NAME
        output.parent.mkdir(exist_ok=True)
        shutil.copy2(apk, output)
        if apk.name.endswith("-unsigned.apk"):
            print("Wear OS release APK is unsigned: set ANDROID_KEYSTORE_PATH/ANDROID_KEY_ALIAS/ANDROID_KEYSTORE_PASSWORD (the phone app's key) to sign it.", file=sys.stderr)
        print(f"Wear OS APK: {output}")
        return output
    for command in watchos_build_commands(root, name, args.debug):
        print(shlex.join(command), flush=True)
        if not args.dry_run: run(command, cwd=root, check=True)
    if args.dry_run:
        print(f"embed {WATCH_APP_NAME} into dist/<app>.app/Watch/")
        return None
    product = watchos_product(root, name, args.debug)
    if not product.is_dir(): raise RuntimeError(f"watchOS build produced no app: {product}")
    if target.simulator:
        embedded = embed_watch_app(product, staged_ios_app(root))
        for command in simulator_adhoc_sign_commands(staged_ios_app(root), embedded):
            run(command, cwd=root, check=True)
    elif args.no_sign:
        embedded = embed_watch_app(product, staged_ios_app(root))
    else:
        # The signer re-stages the iOS app, embeds the watch app and signs
        # the inner app with its own profile before the outer one.
        package_ios(root, sign_kind="distribution", debug_mode=args.debug, watch_app=product)
        return product
    print(f"Embedded watch app: {embedded}")
    return embedded

def expand_targets(value):
    names = [name.strip() for name in value.split(",") if name.strip()]
    if not names:
        raise ValueError("--target requires a target name")
    result = []
    for name in names:
        if name == "all": selected = [n for n, t in TARGETS.items() if not t.simulator]
        elif name == "sims": selected = [n for n, t in TARGETS.items() if t.simulator]
        elif name in TARGETS: selected = [name]
        else: raise ValueError(f"Unknown target {name!r}; use --target list")
        result.extend(n for n in selected if n not in result)
    return result

def parse_args(argv):
    p = argparse.ArgumentParser(description="Build frontend targets. Apple targets need macOS/Xcode; Windows needs a Windows worker.")
    p.add_argument("--target", default="web", help="Target or comma-separated list; list, all (no simulators), all,sims. Aggregate groups skip unavailable workers; explicit targets must build.")
    p.add_argument("--list-targets", action="store_true", help="List all targets and Docker availability without building.")
    p.add_argument("--docker", action="store_true", help="Use Docker for web/Linux/Android; Apple/Windows use native workers.")
    p.add_argument("--docker-image", help="Use an existing compatible builder image instead of building the supplied Dockerfile.")
    p.add_argument("--release", action="store_true", help="Optimized build (the default).")
    p.add_argument("--debug", action="store_true", help="Build debug artifacts.")
    p.add_argument("--final", action="store_true", help="Finalize release artifacts: macOS notarization, iOS distribution IPA, Android release-key signing. No store uploads.")
    p.add_argument("--no-sign", action="store_true", help="Skip Apple signing; iOS produces an unsigned app bundle.")
    p.add_argument("--max-size", action="store_true", help="Use maximum-size optimization from the existing builder.")
    p.add_argument("--package-type", choices=("apk", "aab"), default="apk", help="Android package format (default: apk).")
    p.add_argument("--dry-run", action="store_true", help="Print build plans without modifying files or starting containers.")
    args = p.parse_args(argv)
    if args.final and (args.debug or args.no_sign): p.error("--final cannot be combined with --debug or --no-sign")
    if args.debug and args.release: p.error("choose --debug or --release")
    if args.docker_image and not args.docker: p.error("--docker-image requires --docker")
    return args

def docker_commands(root, name, args):
    target = TARGETS[name]
    # Wasm output is architecture independent; avoid emulating the build tools.
    arch = ("arm64" if platform.machine().lower() in {"arm64", "aarch64"} else "amd64") if target.platform == "web" else target.container_arch
    image = args.docker_image or f"seds-gs-ui-builder:{target.docker}-{arch}"
    commands = []
    if not args.docker_image:
        commands.append(["docker", "buildx", "build", "--load", "--platform", f"linux/{arch}", "--target", target.docker, "-f", str(root / "Dockerfile.builder"), "-t", image, str(root)])
    commands.append(["docker", "run", "--rm", "--platform", f"linux/{arch}",
        "--user", f"{getattr(os, 'getuid', lambda: 1000)()}:{getattr(os, 'getgid', lambda: 1000)()}",
        "--mount", f"type=bind,source={root},target=/work", "--workdir", "/work",
        "--env", "HOME=/tmp/ui-home", "--env", "CARGO_HOME=/opt/cargo",
        "--env", "CARGO_TARGET_DIR=/work/target/docker/" + name,
        "--env", "GRADLE_USER_HOME=/work/target/docker/gradle",
        image, "python3", "build.py", "--target", name,
        "--debug" if args.debug else "--release", "--no-sign", "--package-type", args.package_type,
        *(["--max-size"] if args.max_size else [])])
    return commands

def native_requirement(target):
    system = platform.system()
    if target.platform in {"macos", "ios"} and system != "Darwin": return "requires a macOS worker with Xcode"
    if target.platform == "windows" and system != "Windows": return "requires a Windows worker with MSVC and WebView2 tooling"
    if target.platform == "wearos" and not (os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")):
        return "requires the Android SDK (set ANDROID_HOME) and JDK 17/21; or add --docker"
    if target.platform == "linux" and system != "Linux": return "requires Linux; add --docker"
    if target.platform == "linux":
        host = "aarch64" if platform.machine().lower() in {"aarch64", "arm64"} else "x86_64"
        if not target.triple.startswith(host): return "requires the matching Linux architecture; add --docker (with Docker emulation enabled)"
    return None

def handle_target_args(argv, root, build_frontend, package_ios, finalize_release=None):
    if "--final" in argv and argv and argv[0] in TARGETS:
        argv = ["--target", argv[0], *argv[1:]]
    if not any(a in {"--docker", "--list-targets", "--target", "--final"} or a.startswith("--target=") for a in argv): return False
    if argv and argv[0] == "build": argv = argv[1:]
    args = parse_args(argv)
    if args.list_targets or args.target == "list":
        print("Target             Rust target                      Builder")
        for name, target in TARGETS.items():
            runner = "Docker/native" if target.docker else "macOS/Xcode" if target.platform in {"macos", "ios"} else "Windows/MSVC"
            rust = target.triple or "(none: Kotlin only)"
            watch = f" + {target.watch} app [{' '.join(target.watch_archs)}]" if target.watch else ""
            print(f"{name:18} {rust:32} {runner}" + (" (simulator)" if target.simulator else "") + watch)
        return True
    try: names = expand_targets(args.target)
    except ValueError as error: raise SystemExit(str(error)) from error
    # Explicit names remain mandatory, even when also selected through all/sims.
    explicit = {name.strip() for name in args.target.split(",") if name.strip() in TARGETS}
    failures = []
    skipped = []
    for name in names:
        target = TARGETS[name]
        container = args.docker and target.docker is not None
        problem = None if container else native_requirement(target)
        if problem is None and not container and not args.dry_run and target.watch:
            problem = watch_tool_problem(name)
        print(f"\nTarget {name}: {'Docker' if container else 'native'} · {target.triple or target.watch}", flush=True)
        if problem:
            if name in explicit:
                print(f"Unavailable: {problem}", file=sys.stderr)
                failures.append(name)
            else:
                print(f"Skipped unavailable aggregate target: {problem}", file=sys.stderr)
                skipped.append(name)
            continue
        try:
            final = args.final and not target.simulator and target.platform in {"macos", "ios", "android"}
            if args.final and target.watch:
                raise RuntimeError("--final is not supported for watch targets yet; build ios/android with --final and the watch target separately")
            if final and not args.dry_run:
                if finalize_release is None: raise RuntimeError("Release finalizer is unavailable")
                finalize_release(root, target.platform, args.package_type, prepare=True)
            if container:
                for command in docker_commands(root, name, args):
                    print(shlex.join(command), flush=True)
                    if not args.dry_run: subprocess.run(command, cwd=root, check=True)
            elif args.dry_run:
                if target.triple:
                    print(f"build_frontend(platform={target.platform}, target={target.triple}, release={not args.debug}, sign={not args.no_sign})")
                if target.watch: build_watch(root, name, args)
            else:
                if target.triple:
                    subprocess.run(["rustup", "target", "add", target.triple], cwd=root, check=True)
                    build_frontend(root, platform_name=target.platform, rust_target=target.triple,
                        debug_mode=args.debug, max_size=args.max_size, android_package_type=args.package_type,
                        sign_macos=not args.no_sign)
                if target.watch:
                    build_watch(root, name, args, package_ios=package_ios)
                elif target.platform == "ios" and not target.simulator and not args.no_sign:
                    package_ios(root, sign_kind="distribution", debug_mode=args.debug)
            if final:
                if args.dry_run: print(f"Finalize {target.platform}: distribution signing/verification" + (" + notarization/stapling/Gatekeeper" if target.platform == "macos" else ""))
                else: finalize_release(root, target.platform, args.package_type, prepare=False)
            if not args.dry_run:
                artifact = root / "artifacts" / name
                artifact.mkdir(parents=True, exist_ok=True)
                if (root / "dist").exists(): shutil.copytree(root / "dist", artifact / "dist", dirs_exist_ok=True)
                print(f"Artifacts: {artifact}")
        except (OSError, RuntimeError, subprocess.CalledProcessError, SystemExit) as error:
            print(f"FAILED {name}: {error}", file=sys.stderr)
            failures.append(name)
    if skipped: print("Skipped unavailable targets: " + ", ".join(skipped))
    if failures: raise SystemExit("Targets failed/unavailable: " + ", ".join(failures))
    return True

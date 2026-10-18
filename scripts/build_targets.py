"""Explicit frontend targets and isolated Docker builds. No flashing or deployment."""
import argparse
from dataclasses import dataclass
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys

@dataclass(frozen=True)
class Target:
    platform: str
    triple: str
    simulator: bool = False
    docker: str | None = None
    container_arch: str = "amd64"

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
    "ios_sim": Target("ios", "aarch64-apple-ios-sim", simulator=True),
    "ios_sim_intel": Target("ios", "x86_64-apple-ios", simulator=True),
    "android_sim": Target("android", "x86_64-linux-android", simulator=True, docker="android"),
}

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
    p.add_argument("--target", default="web", help="Target or comma-separated list; list, all (no simulators), all,sims.")
    p.add_argument("--list-targets", action="store_true", help="List all targets and Docker availability without building.")
    p.add_argument("--docker", action="store_true", help="Use Docker for web/Linux/Android; Apple/Windows use native workers.")
    p.add_argument("--docker-image", help="Use an existing compatible builder image instead of building the supplied Dockerfile.")
    p.add_argument("--release", action="store_true", help="Optimized build (the default).")
    p.add_argument("--debug", action="store_true", help="Build debug artifacts.")
    p.add_argument("--no-sign", action="store_true", help="Skip Apple signing; iOS produces an unsigned app bundle.")
    p.add_argument("--max-size", action="store_true", help="Use maximum-size optimization from the existing builder.")
    p.add_argument("--package-type", choices=("apk", "aab"), default="apk", help="Android package format (default: apk).")
    p.add_argument("--dry-run", action="store_true", help="Print build plans without modifying files or starting containers.")
    args = p.parse_args(argv)
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
    if target.platform == "linux" and system != "Linux": return "requires Linux; add --docker"
    if target.platform == "linux":
        host = "aarch64" if platform.machine().lower() in {"aarch64", "arm64"} else "x86_64"
        if not target.triple.startswith(host): return "requires the matching Linux architecture; add --docker (with Docker emulation enabled)"
    return None

def handle_target_args(argv, root, build_frontend, package_ios):
    if not any(a in {"--docker", "--list-targets", "--target"} or a.startswith("--target=") for a in argv): return False
    if argv and argv[0] == "build": argv = argv[1:]
    args = parse_args(argv)
    if args.list_targets or args.target == "list":
        print("Target             Rust target                      Builder")
        for name, target in TARGETS.items():
            runner = "Docker/native" if target.docker else "macOS/Xcode" if target.platform in {"macos", "ios"} else "Windows/MSVC"
            print(f"{name:18} {target.triple:32} {runner}" + (" (simulator)" if target.simulator else ""))
        return True
    try: names = expand_targets(args.target)
    except ValueError as error: raise SystemExit(str(error)) from error
    failures = []
    for name in names:
        target = TARGETS[name]
        container = args.docker and target.docker is not None
        problem = None if container else native_requirement(target)
        print(f"\nTarget {name}: {'Docker' if container else 'native'} · {target.triple}", flush=True)
        if problem:
            print(f"Unavailable: {problem}", file=sys.stderr)
            failures.append(name)
            continue
        try:
            if container:
                for command in docker_commands(root, name, args):
                    print(shlex.join(command), flush=True)
                    if not args.dry_run: subprocess.run(command, cwd=root, check=True)
            elif args.dry_run:
                print(f"build_frontend(platform={target.platform}, target={target.triple}, release={not args.debug}, sign={not args.no_sign})")
            else:
                subprocess.run(["rustup", "target", "add", target.triple], cwd=root, check=True)
                build_frontend(root, platform_name=target.platform, rust_target=target.triple,
                    debug_mode=args.debug, max_size=args.max_size, android_package_type=args.package_type,
                    sign_macos=not args.no_sign)
                if target.platform == "ios" and not target.simulator and not args.no_sign:
                    package_ios(root, sign_kind="distribution", debug_mode=args.debug)
                artifact = root / "artifacts" / name
                artifact.mkdir(parents=True, exist_ok=True)
                if (root / "dist").exists(): shutil.copytree(root / "dist", artifact / "dist", dirs_exist_ok=True)
                print(f"Artifacts: {artifact}")
        except (OSError, subprocess.CalledProcessError, SystemExit) as error:
            print(f"FAILED {name}: {error}", file=sys.stderr)
            failures.append(name)
    if failures: raise SystemExit("Targets failed/unavailable: " + ", ".join(failures))
    return True

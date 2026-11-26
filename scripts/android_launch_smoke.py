#!/usr/bin/env python3
"""Install an APK on a running emulator, launch it and check it stays up.

CI runs this inside an x86_64 emulator (platforms.yml and release.yml,
`android-launch`). A build can succeed and still not start; this catches the
usual reasons:

* the process dies or never stays up (`pidof`), or `am start -W` fails;
* a Java crash (`FATAL EXCEPTION`), a `java.lang.*Error` such as
  UnsatisfiedLinkError / NoClassDefFoundError / NoSuchMethodError (a missing or
  stripped `libmain.so`, wrong ABI, JNI class or method renamed by R8);
* a Rust panic (the app's `GS26` logcat lines, src/debug_log.rs);
* a native crash (`Fatal signal`, the `DEBUG` crash dump, SIGSEGV / SIGABRT);
* an ANR, or the launcher activity not being the resumed one;
* nothing rendered: the app's "[startup] app rendered" line is missing, or
  the screenshot is one flat colour (a blank web view).

Usage:
  android_launch_smoke.py --apk DIR|FILE --component PKG/ACTIVITY --out DIR
                          [--wait SECONDS] [--require-render-log] [--no-screen-check]

Writes logcat.txt, activities.txt, am-start.txt, screenshot.png and
report.json to --out; exits 1 with a list of problems if any check fails.
The analysis functions take plain text so tests can feed them fixtures.
"""
from __future__ import annotations

import argparse
import io
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

RENDER_LOG_LINE = "[startup] app rendered"  # src/app.rs APP_RENDERED_LOG_LINE
APP_LOG_TAG = "GS26"  # src/debug_log.rs ANDROID_LOG_TAG

# `logcat -v threadtime`: date time pid tid level tag: message
LOGCAT_LINE = re.compile(r"^\S+\s+\S+\s+(\d+)\s+(\d+)\s+([VDIWEFA])\s+(.*?)\s*: (.*)$")
JAVA_ERROR = re.compile(r"\bjava\.lang\.(\w*Error)\b|\b(UnsatisfiedLinkError)\b")
NATIVE_SIGNAL = re.compile(r"\b(SIGSEGV|SIGABRT|SIGBUS|SIGILL|SIGFPE|SIGTRAP)\b")


def parse_logcat(text):
    """[(pid, tag, level, message)] for each threadtime line; others are skipped."""
    rows = []
    for line in text.splitlines():
        match = LOGCAT_LINE.match(line.strip())
        if match:
            rows.append((int(match.group(1)), match.group(4).strip(), match.group(3), match.group(5)))
    return rows


def app_pids(rows, package, extra=()):
    """Every pid the system started for the package (a crash restart gets a new one)."""
    pids = set(extra)
    start = re.compile(r"Start proc (\d+):" + re.escape(package) + r"(?::[\w.]+)?/")
    for _pid, _tag, _level, message in rows:
        found = start.search(message)
        if found:
            pids.add(int(found.group(1)))
    return pids


def crash_findings(text, package, pids=()):
    """Problems in a logcat dump that belong to the package's processes."""
    rows = parse_logcat(text)
    pids = app_pids(rows, package, pids)
    findings = []

    def add(problem):
        if problem not in findings:
            findings.append(problem)

    in_fatal = False
    for pid, tag, _level, message in rows:
        ours = pid in pids
        if tag == "AndroidRuntime":
            if message.startswith("FATAL EXCEPTION"):
                in_fatal = ours
                if ours:
                    add(f"Java crash: {message}")
                continue
            if re.match(r"Process: " + re.escape(package) + r"(?::[\w.]+)?, PID: \d+", message):
                in_fatal = True
                add(f"Java crash: {message}")
                continue
            if in_fatal and (found := JAVA_ERROR.search(message)):
                add(f"Java crash cause: {message.strip()}")
                continue
        if ours and (found := JAVA_ERROR.search(message)):
            add(f"{found.group(1) or found.group(2)} ({tag}): {message.strip()}")
        if ours and tag == APP_LOG_TAG and message.startswith("[panic]"):
            add(f"Rust panic: {message.strip()}")
        elif ours and "panicked at" in message:
            add(f"Rust panic ({tag}): {message.strip()}")
        if tag == "libc" and message.startswith("Fatal signal") and (
                ours or re.search(r"\(" + re.escape(package[:15]), message)):
            add(f"Native crash: {message.strip()}")
        if tag == "DEBUG" and f">>> {package}" in message:
            add(f"Native crash dump: {message.strip()}")
        elif ours and tag != "libc" and (found := NATIVE_SIGNAL.search(message)):
            add(f"Native crash ({found.group(1)}, {tag}): {message.strip()}")
        if tag == "ActivityManager":
            if re.search(r"Process " + re.escape(package) + r"(?::[\w.]+)? \(pid \d+\) has died", message):
                add(f"Process died: {message.strip()}")
            if message.startswith(f"ANR in {package}"):
                add(f"ANR: {message.strip()}")
            if re.search(r"Force finishing activity " + re.escape(package) + "/", message):
                add(f"Activity force-finished: {message.strip()}")
    return findings


def render_logged(text, package, pids=()):
    """True when one of the package's processes logged the app-rendered line."""
    rows = parse_logcat(text)
    pids = app_pids(rows, package, pids)
    return any(tag == APP_LOG_TAG and RENDER_LOG_LINE in message and (pid in pids or not pids)
               for pid, tag, _level, message in rows)


def component_patterns(component):
    package, _, activity = component.partition("/")
    full = package + activity if activity.startswith(".") else activity
    names = {full}
    if full.startswith(package + "."):
        names.add(full[len(package):])
    return [re.escape(package + "/" + name) for name in names]


def activity_resumed(dumpsys, component):
    """True if `dumpsys activity activities` shows the component as the resumed activity."""
    names = "|".join(component_patterns(component))
    pattern = re.compile(r"(?:topResumedActivity|mResumedActivity|ResumedActivity)\s*[:=]\s*"
                         r"ActivityRecord\{[^}]*?\s(?:" + names + r")[\s}]")
    return bool(pattern.search(dumpsys))


def am_start_ok(output):
    """`am start -W` printed Status: ok and no error."""
    return bool(re.search(r"^Status: ok\s*$", output, re.MULTILINE)) and not re.search(
        r"^(?:Error|Exception|Warning: Activity not started)", output, re.MULTILINE)


def screenshot_problem(png_bytes):
    """None if the middle of the screen has visible content, else why not.

    The top and bottom are cut off: system bars draw icons over an otherwise
    blank window."""
    try:
        from PIL import Image
    except ImportError:
        return "Pillow is not installed; cannot check the screenshot"
    try:
        image = Image.open(io.BytesIO(png_bytes)).convert("RGB")
    except Exception as error:  # noqa: BLE001 - any decoder failure means no usable screenshot
        return f"screenshot is not a readable image ({error})"
    width, height = image.size
    if width < 50 or height < 50:
        return f"screenshot is {width}x{height}"
    middle = image.crop((width // 20, height * 3 // 20, width * 19 // 20, height * 17 // 20))
    middle = middle.resize((max(1, middle.width // 4), max(1, middle.height // 4)))
    colours = middle.getcolors(maxcolors=middle.width * middle.height) or []
    total = sum(count for count, _ in colours)
    dominant = max((count for count, _ in colours), default=0)
    if total == 0 or dominant / total > 0.985 or len(colours) < 8:
        return (f"screen is blank: {len(colours)} colours, the most common covers "
                f"{100 * dominant / max(total, 1):.1f}% of the middle")
    return None


def evaluate(*, logcat, package, component, pids_alive, am_start, dumpsys, png, require_render_log,
             check_screen=True):
    """All problems found for one launch, as readable sentences."""
    problems = []
    if not am_start_ok(am_start):
        problems.append("am start -W did not report Status: ok:\n" + am_start.strip())
    if not pids_alive:
        problems.append(f"{package} is not running at the end of the wait (pidof found nothing)")
    problems.extend(crash_findings(logcat, package, pids_alive))
    if not activity_resumed(dumpsys, component):
        problems.append(f"{component} is not the resumed activity")
    if require_render_log and not render_logged(logcat, package, pids_alive):
        problems.append(f"the app never logged {RENDER_LOG_LINE!r} ({APP_LOG_TAG} tag)")
    if not check_screen:
        pass
    elif png is None:
        problems.append("no screenshot")
    elif (reason := screenshot_problem(png)):
        problems.append(reason)
    return problems


# --- emulator side --------------------------------------------------------

def adb_path():
    for base in [os.environ.get("ANDROID_HOME"), os.environ.get("ANDROID_SDK_ROOT")]:
        if base and (Path(base) / "platform-tools" / "adb").is_file():
            return str(Path(base) / "platform-tools" / "adb")
    found = shutil.which("adb")
    if not found:
        raise RuntimeError("adb not found")
    return found


def adb(*args, check=True, timeout=120, binary=False):
    result = subprocess.run([adb_path(), *args], capture_output=True, timeout=timeout)
    if check and result.returncode:
        raise RuntimeError(f"adb {' '.join(args)} failed ({result.returncode}): "
                           + result.stderr.decode(errors="replace").strip())
    return result.stdout if binary else result.stdout.decode(errors="replace")


def shell(*args, **kwargs):
    return adb("shell", *args, **kwargs)


def wait_for_boot(timeout=600):
    adb("wait-for-device", timeout=timeout)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if shell("getprop", "sys.boot_completed", check=False).strip() == "1":
            return
        time.sleep(3)
    raise RuntimeError("emulator did not finish booting")


def running_pids(package):
    out = shell("pidof", package, check=False).split()
    if not out:  # Older toybox: fall back to ps.
        listing = shell("ps", "-A", check=False) or shell("ps", check=False)
        out = [line.split()[1] for line in listing.splitlines()
               if line.split() and line.split()[-1] == package and len(line.split()) > 1]
    return {int(pid) for pid in out if pid.isdigit()}


def find_apk(path):
    path = Path(path)
    if path.is_file():
        return path
    apks = sorted(p for p in path.rglob("*.apk"))
    if len(apks) != 1:
        raise RuntimeError(f"expected exactly one .apk under {path}, found {[str(p) for p in apks]}")
    return apks[0]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--apk", required=True, help="APK file, or a directory holding exactly one")
    parser.add_argument("--component", required=True, help="package/activity of the launcher activity")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--wait", type=int, default=45, help="seconds to watch the app after launch")
    parser.add_argument("--require-render-log", action="store_true",
                        help=f"require the app's {RENDER_LOG_LINE!r} logcat line")
    parser.add_argument("--no-screen-check", action="store_true",
                        help="keep the screenshot but do not require visible content (an emulator image "
                             "whose built-in WebView is too old to stand for real devices)")
    args = parser.parse_args(argv)
    package = args.component.split("/")[0]
    args.out.mkdir(parents=True, exist_ok=True)
    apk = find_apk(args.apk)
    print(f"APK: {apk} ({apk.stat().st_size} bytes)", flush=True)

    wait_for_boot()
    sdk = shell("getprop", "ro.build.version.sdk").strip()
    abi = shell("getprop", "ro.product.cpu.abi").strip()
    print(f"Emulator: API {sdk}, {abi}", flush=True)
    shell("input", "keyevent", "82", check=False)  # dismiss the lock screen
    adb("install", "-r", "-g", str(apk), timeout=600)
    adb("logcat", "-b", "all", "-c", check=False)

    am_start = shell("am", "start", "-W", "-n", args.component, check=False, timeout=180)
    (args.out / "am-start.txt").write_text(am_start)
    print(am_start, flush=True)

    deadline = time.monotonic() + args.wait
    pids = running_pids(package)
    while time.monotonic() < deadline:
        time.sleep(5)
        pids = running_pids(package)
        if not pids:
            print(f"{package} is no longer running", flush=True)
            break

    png = None
    try:
        png = adb("exec-out", "screencap", "-p", binary=True, timeout=60)
        (args.out / "screenshot.png").write_bytes(png)
    except (RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"screenshot failed: {error}", flush=True)
    dumpsys = shell("dumpsys", "activity", "activities", check=False, timeout=60)
    (args.out / "activities.txt").write_text(dumpsys)
    logcat = adb("logcat", "-d", "-b", "main,system,crash", "-v", "threadtime", check=False, timeout=120)
    (args.out / "logcat.txt").write_text(logcat)

    problems = evaluate(logcat=logcat, package=package, component=args.component, pids_alive=pids,
                        am_start=am_start, dumpsys=dumpsys, png=png, require_render_log=args.require_render_log,
                        check_screen=not args.no_screen_check)
    report = {"apk": apk.name, "component": args.component, "api": sdk, "abi": abi,
              "wait_seconds": args.wait, "pids": sorted(pids), "problems": problems}
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    if problems:
        print(f"LAUNCH FAILED on API {sdk} ({abi}):", file=sys.stderr)
        for problem in problems:
            print(f"- {problem}", file=sys.stderr)
        return 1
    print(f"Launch OK on API {sdk} ({abi}): {args.component} running (pid {sorted(pids)}), resumed, rendered")
    return 0


if __name__ == "__main__":
    sys.exit(main())

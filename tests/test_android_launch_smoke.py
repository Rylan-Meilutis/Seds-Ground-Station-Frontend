"""Launch smoke test analysis (scripts/android_launch_smoke.py) on logcat fixtures."""
import importlib.util
import itertools
import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).parents[1]
spec = importlib.util.spec_from_file_location("android_launch_smoke", ROOT / "scripts/android_launch_smoke.py")
smoke = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = smoke
spec.loader.exec_module(smoke)

PKG = "com.UBSEDS.GS26"
COMPONENT = f"{PKG}/dev.dioxus.main.MainActivity"
WEAR = f"{PKG}/com.ubseds.gs26.wear.MainActivity"
APP = 4242


def line(pid, tag, message, level="I", tid=None):
    return f"11-26 07:00:16.273  {pid:5d} {tid or pid:5d} {level} {tag}: {message}"


START = line(510, "ActivityManager", f"Start proc {APP}:{PKG}/u0a210 for next-top-activity {{{COMPONENT}}}")
RENDERED = line(APP, "GS26", "[startup] app rendered")
HEALTHY = "\n".join([
    "--------- beginning of main",
    START,
    line(APP, "GS26", "[startup] native main entered"),
    line(APP, "GS26", "[startup] launching desktop app"),
    RENDERED,
    # Other processes' trouble is not ours.
    line(999, "AndroidRuntime", "FATAL EXCEPTION: main", "E"),
    line(999, "AndroidRuntime", "Process: com.android.systemui, PID: 999", "E"),
    line(999, "AndroidRuntime", "java.lang.NullPointerException: boom", "E"),
    line(998, "libc", "Fatal signal 11 (SIGSEGV), code 1, fault addr 0x0 in tid 998 (surfaceflinger), pid 998 (surfaceflinger)", "F"),
    line(997, "SomeService", "java.lang.OutOfMemoryError: elsewhere", "E"),
])

FAILURES = {
    "java crash": [
        line(APP, "AndroidRuntime", "FATAL EXCEPTION: main", "E"),
        line(APP, "AndroidRuntime", f"Process: {PKG}, PID: {APP}", "E"),
        line(APP, "AndroidRuntime", "java.lang.RuntimeException: Unable to start activity", "E"),
    ],
    "missing libmain.so": [
        line(APP, "AndroidRuntime", "FATAL EXCEPTION: main", "E"),
        line(APP, "AndroidRuntime", f"Process: {PKG}, PID: {APP}", "E"),
        line(APP, "AndroidRuntime", 'java.lang.UnsatisfiedLinkError: dlopen failed: library "libmain.so" not found', "E"),
    ],
    "JNI class renamed by R8": [
        line(APP, "art", "JNI DETECTED ERROR IN APPLICATION: java.lang.NoClassDefFoundError: dev/dioxus/main/RustWebView", "F"),
    ],
    "JNI method missing": [
        line(APP, "System.err", "java.lang.NoSuchMethodError: no non-static method \"Ldev/dioxus/main/WryActivity;.onWebViewCreate\"", "W"),
    ],
    "caught link error": [
        line(APP, "VoiceAudioBridge", "native side not loaded java.lang.UnsatisfiedLinkError: No implementation found", "W"),
    ],
    "rust panic": [
        line(APP, "GS26", "[panic] location=src/app.rs:10 payload=called `Option::unwrap()` on a `None` value", "E"),
    ],
    "rust default panic": [
        line(APP, "RustStdoutStderr", "thread 'main' panicked at src/main.rs:5:1:", "E"),
    ],
    "native crash": [
        line(APP, "libc", f"Fatal signal 6 (SIGABRT), code -1 (SI_QUEUE) in tid {APP} (main), pid {APP} ({PKG})", "F"),
    ],
    "native crash dump": [
        line(4300, "DEBUG", f"pid: {APP}, tid: {APP}, name: main  >>> {PKG} <<<", "F"),
        line(4300, "DEBUG", "signal 11 (SIGSEGV), code 1 (SEGV_MAPERR), fault addr 0x0", "F"),
    ],
    "signal in app log": [
        line(APP, "crash_dump", "received SIGSEGV in libmain.so", "F"),
    ],
    "process died": [
        line(510, "ActivityManager", f"Process {PKG} (pid {APP}) has died: fg  TOP", "I"),
    ],
    "anr": [
        line(510, "ActivityManager", f"ANR in {PKG} ({COMPONENT})", "E"),
    ],
    "force finished": [
        line(510, "ActivityManager", f"  Force finishing activity {COMPONENT}", "W"),
    ],
}

RESUMED_36 = f"""
  Display #0 (activities from top to bottom):
    * Task{{a1b2 #12 type=standard A={PKG} U=0 visible=true}}
  ResumedActivity: ActivityRecord{{1e2f u0 {COMPONENT} t12}}
  topResumedActivity=ActivityRecord{{1e2f u0 {COMPONENT} t12}}
"""
RESUMED_24 = f"""
    mResumedActivity: ActivityRecord{{8c8 u0 {PKG}/dev.dioxus.main.MainActivity t5}}
"""
PAUSED_BEHIND_DIALOG = f"""
  ResumedActivity: ActivityRecord{{77 u0 com.google.android.permissioncontroller/com.android.packageinstaller.permission.ui.GrantPermissionsActivity t12}}
    Hist #1: ActivityRecord{{1e2f u0 {COMPONENT} t12}}
"""
AM_OK = f"Starting: Intent {{ cmp={COMPONENT} }}\nStatus: ok\nLaunchState: COLD\nActivity: {COMPONENT}\nTotalTime: 812\nWaitTime: 815\nComplete\n"
AM_ERROR = f"Starting: Intent {{ cmp={COMPONENT} }}\nError type 3\nError: Activity class {{{COMPONENT}}} does not exist.\n"


def png(content=True, size=(360, 760)):
    from PIL import Image, ImageDraw
    image = Image.new("RGB", size, (2, 6, 23))
    draw = ImageDraw.Draw(image)
    draw.rectangle((0, 0, size[0], 30), fill=(40, 40, 40))  # status bar
    draw.text((10, 8), "12:00", fill=(255, 255, 255))
    if content:
        for row in range(8):
            draw.rectangle((30, 160 + row * 50, 330, 190 + row * 50), fill=(30 + row * 20, 90, 200 - row * 10))
            draw.text((40, 168 + row * 50), f"Station {row}", fill=(255, 255, 255))
    buffer = io.BytesIO()
    image.save(buffer, format="PNG")
    return buffer.getvalue()


class LogcatAnalysis(unittest.TestCase):
    def test_healthy_launch_has_no_findings_and_ignores_other_processes(self):
        self.assertEqual(smoke.crash_findings(HEALTHY, PKG), [])
        self.assertTrue(smoke.render_logged(HEALTHY, PKG))
        self.assertEqual(smoke.app_pids(smoke.parse_logcat(HEALTHY), PKG), {APP})

    def test_every_failure_class_is_reported(self):
        for name, lines in FAILURES.items():
            with self.subTest(name):
                text = "\n".join([START, *lines])
                self.assertTrue(smoke.crash_findings(text, PKG), name)

    def test_pid_from_pidof_counts_without_a_start_line(self):
        text = line(APP, "GS26", "[panic] location=x payload=y", "E")
        self.assertEqual(smoke.crash_findings(text, PKG), [])
        self.assertEqual(len(smoke.crash_findings(text, PKG, {APP})), 1)

    def test_a_restarted_process_is_still_ours(self):
        text = "\n".join([START, line(510, "ActivityManager", f"Start proc 5000:{PKG}/u0a210 for service"),
                          line(5000, "GS26", "[panic] second process", "E")])
        self.assertEqual(len(smoke.crash_findings(text, PKG)), 1)

    def test_a_deliberate_restart_is_not_a_death_but_a_loop_is(self):
        restart = line(APP, "GS26", smoke.RESTART_LOG_LINE)
        died = line(510, "ActivityManager", f"Process {PKG} (pid {APP}) has died: fg  TOP", "I")
        second = line(510, "ActivityManager", f"Start proc 5000:{PKG}/u0a210 for top-activity")
        self.assertEqual(smoke.crash_findings("\n".join([START, restart, died, second]), PKG), [])
        # The same death without the marker, or a marker from another app, still fails.
        self.assertTrue(smoke.crash_findings("\n".join([START, died, second]), PKG))
        other = line(999, "GS26", smoke.RESTART_LOG_LINE)
        self.assertTrue(smoke.crash_findings("\n".join([START, other, died]), PKG))
        loop = [START]
        for pid in (APP, 5001, 5002):
            loop += [line(510, "ActivityManager", f"Start proc {pid}:{PKG}/u0a210 for top-activity"),
                     line(pid, "GS26", smoke.RESTART_LOG_LINE)]
        self.assertTrue(any("restart loop" in f for f in smoke.crash_findings("\n".join(loop), PKG)))

    def test_restart_marker_matches_the_activity(self):
        self.assertIn(smoke.RESTART_LOG_LINE, (ROOT / "build.py").read_text())
        manifest = (ROOT / "platform/android/AndroidManifest.xml").read_text()
        for change in ("uiMode", "fontScale", "density", "locale", "smallestScreenSize", "orientation"):
            self.assertRegex(manifest, r'android:configChanges="[^"]*\b' + change + r'\b')

    def test_render_line_missing(self):
        self.assertFalse(smoke.render_logged(START, PKG))
        self.assertFalse(smoke.render_logged("\n".join([START, line(999, "GS26", "[startup] app rendered")]), PKG))


class ActivityAndStart(unittest.TestCase):
    def test_resumed_activity_on_new_and_old_android(self):
        self.assertTrue(smoke.activity_resumed(RESUMED_36, COMPONENT))
        self.assertTrue(smoke.activity_resumed(RESUMED_24, COMPONENT))
        self.assertFalse(smoke.activity_resumed(PAUSED_BEHIND_DIALOG, COMPONENT))
        self.assertFalse(smoke.activity_resumed("", COMPONENT))
        wear = f"  topResumedActivity=ActivityRecord{{9 u0 {WEAR} t3}}\n"
        self.assertTrue(smoke.activity_resumed(wear, WEAR))
        self.assertFalse(smoke.activity_resumed(wear, COMPONENT))

    def test_am_start(self):
        self.assertTrue(smoke.am_start_ok(AM_OK))
        self.assertFalse(smoke.am_start_ok(AM_ERROR))
        self.assertFalse(smoke.am_start_ok(""))

    def test_components_match_the_manifests(self):
        import xml.etree.ElementTree as ET
        ns = "{http://schemas.android.com/apk/res/android}"
        phone = ET.parse(ROOT / "platform/android/AndroidManifest.xml").getroot()
        names = [a.get(ns + "name") for a in phone.iter("activity")
                 if any(c.get(ns + "name") == "android.intent.category.LAUNCHER" for c in a.iter("category"))]
        self.assertEqual(names, [COMPONENT.split("/")[1]])
        wear = ET.parse(ROOT / "watch/wear/app/src/main/AndroidManifest.xml").getroot()
        names = [a.get(ns + "name") for a in wear.iter("activity")
                 if any(c.get(ns + "name") == "android.intent.category.LAUNCHER" for c in a.iter("category"))]
        self.assertEqual(names, [".MainActivity"])
        self.assertIn('namespace = "com.ubseds.gs26.wear"', (ROOT / "watch/wear/app/build.gradle.kts").read_text())
        for workflow in ["platforms.yml", "release.yml"]:
            text = (ROOT / ".github/workflows" / workflow).read_text()
            self.assertIn(COMPONENT, text, workflow)
        self.assertIn(WEAR, (ROOT / ".github/workflows/platforms.yml").read_text())

    def test_render_marker_and_log_tag_match_the_app(self):
        self.assertIn(f'APP_RENDERED_LOG_LINE: &str = "{smoke.RENDER_LOG_LINE}"', (ROOT / "src/app.rs").read_text())
        self.assertIn(f'ANDROID_LOG_TAG: &str = "{smoke.APP_LOG_TAG}"', (ROOT / "src/debug_log.rs").read_text())


class NativeMinSdk(unittest.TestCase):
    def test_native_code_links_for_the_manifest_min_sdk(self):
        # dx 0.7.10 links for API 28 unless told otherwise; the manifest says 24.
        import tomllib
        dioxus = tomllib.loads((ROOT / "Dioxus.toml").read_text())
        build = (ROOT / "build.py").read_text()
        self.assertIn('os.environ.get("ANDROID_MIN_SDK", "24")', build)
        self.assertEqual(dioxus["application"]["android_min_sdk_version"], 24)


class MobileLaunchArguments(unittest.TestCase):
    def test_mobile_never_reads_process_arguments(self):
        # std::env::args crashed the Android app at startup (SIGSEGV in
        # args_os): Android and iOS load the app as a library with no command
        # line of its own. Every read of them must be gated to desktop.
        import re
        for path in sorted((ROOT / "src").rglob("*.rs")):
            text = path.read_text()
            for match in re.finditer(r"std::env::args", text):
                head = text[: match.start()]
                fn_start = head.rfind("\nfn ") if head.rfind("\nfn ") > head.rfind("\npub(crate) fn ") else head.rfind("\npub(crate) fn ")
                attrs = head[max(0, fn_start - 300) : fn_start]
                cfg = attrs[attrs.rfind("#[cfg(") :] if "#[cfg(" in attrs else ""
                with self.subTest(file=str(path.relative_to(ROOT))):
                    self.assertIn('target_os = "android"', cfg, "process arguments read without a desktop-only cfg")
                    self.assertIn('target_os = "ios"', cfg)
                    self.assertTrue(cfg.startswith("#[cfg(not("), cfg)


class Screenshot(unittest.TestCase):
    def test_content_passes_and_blank_fails(self):
        self.assertIsNone(smoke.screenshot_problem(png(True)))
        self.assertIn("blank", smoke.screenshot_problem(png(False)))
        self.assertIn("not a readable image", smoke.screenshot_problem(b"not a png"))
        self.assertIn("is 10x10", smoke.screenshot_problem(png(False, (10, 10))))


class Evaluate(unittest.TestCase):
    def run_eval(self, **changes):
        values = dict(logcat="\n".join([START, RENDERED]), package=PKG, component=COMPONENT, pids_alive={APP},
                      am_start=AM_OK, dumpsys=RESUMED_36, png=png(True), require_render_log=True)
        values.update(changes)
        return smoke.evaluate(**values)

    def test_healthy(self):
        self.assertEqual(self.run_eval(), [])

    def test_each_check_can_fail_on_its_own(self):
        cases = {
            "am start": dict(am_start=AM_ERROR),
            "process gone": dict(pids_alive=set()),
            "crash": dict(logcat="\n".join([START, RENDERED, *FAILURES["missing libmain.so"]])),
            "not resumed": dict(dumpsys=PAUSED_BEHIND_DIALOG),
            "no render line": dict(logcat=START),
            # Android 7 once returned nothing for "-b main,system,crash".
            "empty logcat": dict(logcat="Unable to open log device 'main,system,crash'\n", require_render_log=False),
            "no screenshot": dict(png=None),
            "blank screen": dict(png=png(False)),
        }
        for name, change in cases.items():
            with self.subTest(name):
                self.assertTrue(self.run_eval(**change), name)
        self.assertEqual(self.run_eval(png=png(False), check_screen=False), [])
        self.assertEqual(self.run_eval(png=None, check_screen=False), [])
        # API 24's ancient WebView: only the native launch line is required.
        launched = line(APP, "GS26", smoke.LAUNCH_LOG_LINE)
        self.assertEqual(self.run_eval(logcat="\n".join([START, launched]), require_render_log=False,
                                       require_launch_log=True), [])
        self.assertTrue(self.run_eval(logcat=START, require_render_log=False, require_launch_log=True))
        self.assertIn(smoke.LAUNCH_LOG_LINE, (ROOT / "src/main.rs").read_text())
        # Wear has no Rust and so no render line.
        self.assertEqual(self.run_eval(logcat=START, require_render_log=False), [])


class EmulatorDriver(unittest.TestCase):
    def test_main_collects_artifacts_and_fails_on_a_crash(self):
        crash = "\n".join([START, RENDERED, *FAILURES["native crash"]])
        responses = {
            ("shell", "getprop", "sys.boot_completed"): "1\n",
            ("shell", "pm", "path", "android"): "package:/system/framework/framework-res.apk\n",
            ("shell", "getprop", "ro.build.version.sdk"): "36\n",
            ("shell", "getprop", "ro.product.cpu.abi"): "x86_64\n",
            ("shell", "pidof", PKG): f"{APP}\n",
            ("shell", "dumpsys", "activity", "activities"): RESUMED_36,
            ("shell", "settings", "get", "system", "screen_off_timeout"): "60000\n",
        }
        calls = []

        def fake(args, capture_output, timeout):
            calls.append(tuple(args[1:]))
            key = tuple(args[1:])
            if key[:3] == ("shell", "am", "start"):
                out = AM_OK
            elif key[:2] == ("exec-out", "screencap"):
                return subprocess.CompletedProcess(args, 0, png(True), b"")
            elif key[:2] == ("logcat", "-d"):
                out = crash
            else:
                out = responses.get(key, "")
            return subprocess.CompletedProcess(args, 0, out.encode(), b"")

        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp) / "out"
            apk = Path(temp) / "in" / "dist" / "SEDS GS.apk"
            apk.parent.mkdir(parents=True)
            apk.write_bytes(b"apk")
            with patch.object(smoke, "adb_path", return_value="adb"), \
                    patch.object(smoke.subprocess, "run", side_effect=fake), \
                    patch.object(smoke.time, "sleep"), \
                    patch.object(smoke.time, "monotonic", side_effect=itertools.count(0, 100)), \
                    patch("sys.stdout", io.StringIO()), patch("sys.stderr", io.StringIO()) as err:
                code = smoke.main(["--apk", str(apk.parent.parent), "--component", COMPONENT,
                                   "--out", str(out), "--wait", "30", "--require-render-log"])
            self.assertEqual(code, 1)
            self.assertIn("Native crash", err.getvalue())
            for name in ["logcat.txt", "activities.txt", "am-start.txt", "screenshot.png", "report.json"]:
                self.assertTrue((out / name).is_file(), name)
            self.assertIn(("install", "-r", "-g", str(apk)), calls)
            self.assertIn(("shell", "am", "start", "-W", "-n", COMPONENT), calls)
            self.assertIn(("logcat", "-d", "-b", "main", "-b", "system", "-b", "crash", "-v", "threadtime"), calls)
            # Unplugged before the launch (Wear's charging screen), restored after.
            self.assertLess(calls.index(("shell", "dumpsys", "battery", "unplug")),
                            calls.index(("shell", "am", "start", "-W", "-n", COMPONENT)))
            self.assertIn(("shell", "dumpsys", "battery", "reset"), calls)
            # The display stays awake through the check (API 24 slept after a minute).
            self.assertLess(calls.index(("shell", "settings", "put", "system", "screen_off_timeout",
                                         str(smoke.SCREEN_AWAKE_MS))),
                            calls.index(("shell", "am", "start", "-W", "-n", COMPONENT)))
            self.assertLess(calls.index(("shell", "input", "keyevent", "224")),
                            calls.index(("shell", "am", "start", "-W", "-n", COMPONENT)))
            # ...and gets its own timeout back afterwards.
            self.assertGreater(calls.index(("shell", "settings", "put", "system", "screen_off_timeout", "60000")),
                               calls.index(("shell", "am", "start", "-W", "-n", COMPONENT)))

    def test_a_font_size_change_must_keep_the_process(self):
        healthy = "\n".join([START, RENDERED])
        for restarts, expected in ((False, 0), (True, 1)):
            state = {"scaled": False}

            def fake(args, capture_output, timeout):
                key = tuple(args[1:])
                if key[:5] == ("shell", "settings", "put", "system", "font_scale"):
                    state["scaled"] = True
                if key[:3] == ("shell", "am", "start"):
                    out = AM_OK
                elif key[:2] == ("exec-out", "screencap"):
                    return subprocess.CompletedProcess(args, 0, png(True), b"")
                elif key[:2] == ("logcat", "-d"):
                    out = healthy
                elif key == ("shell", "pidof", PKG):
                    out = "5000\n" if restarts and state["scaled"] else f"{APP}\n"
                elif key == ("shell", "getprop", "sys.boot_completed"):
                    out = "1\n"
                elif key == ("shell", "pm", "path", "android"):
                    out = "package:/system/framework/framework-res.apk\n"
                elif key == ("shell", "settings", "get", "system", "font_scale"):
                    out = "1.0\n"
                elif key == ("shell", "dumpsys", "activity", "activities"):
                    out = RESUMED_36
                else:
                    out = ""
                return subprocess.CompletedProcess(args, 0, out.encode(), b"")

            with tempfile.TemporaryDirectory() as temp, self.subTest(restarts=restarts):
                apk = Path(temp) / "in" / "dist" / "SEDS GS.apk"
                apk.parent.mkdir(parents=True)
                apk.write_bytes(b"apk")
                with patch.object(smoke, "adb_path", return_value="adb"), \
                        patch.object(smoke.subprocess, "run", side_effect=fake), \
                        patch.object(smoke.time, "sleep"), \
                        patch.object(smoke.time, "monotonic", side_effect=itertools.count(0, 100)), \
                        patch("sys.stdout", io.StringIO()), patch("sys.stderr", io.StringIO()) as err:
                    code = smoke.main(["--apk", str(apk.parent.parent), "--component", COMPONENT,
                                       "--out", str(Path(temp) / "out"), "--wait", "30", "--require-render-log"])
                self.assertEqual(code, expected, err.getvalue())
                if restarts:
                    self.assertIn("font-size change", err.getvalue())

    def test_find_apk_needs_exactly_one(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with self.assertRaises(RuntimeError):
                smoke.find_apk(root)
            (root / "a.apk").write_bytes(b"")
            self.assertEqual(smoke.find_apk(root), root / "a.apk")
            (root / "b.apk").write_bytes(b"")
            with self.assertRaises(RuntimeError):
                smoke.find_apk(root)


if __name__ == "__main__":
    unittest.main()


class Install(unittest.TestCase):
    def test_no_streamed_install_before_android_8(self):
        apk = Path("app.apk")
        self.assertEqual(smoke.install_args(apk, 24), ["install", "-r", "-g", "--no-streaming", "app.apk"])
        self.assertEqual(smoke.install_args(apk, 25), ["install", "-r", "-g", "--no-streaming", "app.apk"])
        self.assertEqual(smoke.install_args(apk, 26), ["install", "-r", "-g", "app.apk"])
        self.assertEqual(smoke.install_args(apk, 36), ["install", "-r", "-g", "app.apk"])

    def test_waits_for_package_manager_then_retries_one_stuck_install(self):
        answers = iter(["", "Error: Could not access the Package Manager", "package:/system/framework/framework-res.apk"])
        calls = []

        def adb(*args, **kwargs):
            calls.append(args)
            if len(calls) == 1:
                raise subprocess.TimeoutExpired(["adb", *args], kwargs.get("timeout"))
            return "Success"
        with patch.object(smoke, "shell", side_effect=lambda *a, **k: next(answers)) as shell, \
             patch.object(smoke, "adb", side_effect=adb), patch.object(smoke.time, "sleep"):
            smoke.install(Path("app.apk"), 24)
        self.assertEqual(shell.call_count, 3)
        self.assertEqual(calls, [tuple(smoke.install_args(Path("app.apk"), 24))] * 2)

    def test_second_stuck_install_fails_the_run(self):
        def adb(*args, **kwargs):
            raise subprocess.TimeoutExpired(["adb", *args], 300)
        with patch.object(smoke, "shell", return_value="package:/x"), patch.object(smoke, "adb", side_effect=adb), \
             patch.object(smoke.time, "sleep"):
            with self.assertRaises(subprocess.TimeoutExpired):
                smoke.install(Path("app.apk"), 24)

    def test_package_manager_that_never_starts_fails_the_run(self):
        clock = itertools.count(0, 60)
        with patch.object(smoke, "shell", return_value=""), patch.object(smoke.time, "sleep"), \
             patch.object(smoke.time, "monotonic", side_effect=lambda: next(clock)):
            with self.assertRaisesRegex(RuntimeError, "package manager did not start"):
                smoke.install(Path("app.apk"), 24)

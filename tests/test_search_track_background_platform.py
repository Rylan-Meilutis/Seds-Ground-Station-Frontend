"""Recovery search track in the background: platform contracts
(docs/proximity-lock.md, "Recovery search in the background";
docs/watch-companion.md, "Recovery search").

A track is recorded with the app in the background only while the user
shares it in a running search (their own opt-in) and location is allowed.
These tests check the declarations each platform needs, that the native
code starts only from Rust's gate (or, on the watches, from the recorder's
own opt-in check) and that nothing native sends a location. No SDK, device
or Apple tool is used; the iOS script runs through the test PlistBuddy
adapter.
"""
import importlib.util
from pathlib import Path
import plistlib
import re
import sys
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
ANDROID = "{http://schemas.android.com/apk/res/android}"
KOTLIN = ROOT / "platform/android/kotlin/com/ubseds/gs26"
WEAR = ROOT / "watch/wear/app/src/main"
APPLE_WATCH = ROOT / "watch/apple/GS26Watch"
NETWORK_APIS = ["NSURLSession", "NSURLConnection", "dataTaskWith", "HttpURLConnection", "okhttp", "URL(", "Socket("]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def function_body(source, signature):
    """The text of a Rust function from its signature to its closing brace."""
    start = source.index(signature)
    depth = 0
    for i in range(source.index("{", start), len(source)):
        if source[i] == "{":
            depth += 1
        elif source[i] == "}":
            depth -= 1
            if depth == 0:
                return source[start:i + 1]
    raise AssertionError(f"unterminated {signature}")


class SharedGate(unittest.TestCase):
    def setUp(self):
        self.model = (ROOT / "src/telemetry_dashboard/recovery_search.rs").read_text()
        self.view = (ROOT / "src/telemetry_dashboard/recovery_search_view.rs").read_text()
        self.native = (ROOT / "src/telemetry_dashboard/recovery_search_native.rs").read_text()

    def test_background_needs_sharing_support_and_a_permission(self):
        body = function_body(self.model, "pub(crate) fn background_recording(")
        self.assertIn("mode == Mode::Sharing && supported", body)
        self.assertIn("Permission::Foreground | Permission::Background", body)
        # Sharing itself needs a running search and this device's opt-in.
        mode = function_body(self.model, "pub(crate) fn mode(")
        self.assertIn("s.active", mode)
        self.assertIn("outbox.armed()", mode)

    def test_the_runtime_starts_the_recorder_only_from_the_gate(self):
        self.assertEqual(self.view.count("native::set_running(true)"), 1)
        follow = function_body(self.view, "fn follow_background(")
        self.assertIn("native::set_running(true)", follow)
        self.assertIn("if !want {\n        stop_background();", follow)
        self.assertRegex(self.view, r"let background = model::background_recording\(mode, native::SUPPORTED, permission\);\s*follow_background\(background, now\);")
        # Stopping sharing stops it at once, and so does unmounting.
        self.assertIn("stop_background();", function_body(self.view, "async fn stop_sharing("))
        self.assertIn("use_drop(stop_background);", self.view)
        # Background fixes take the foreground path into the outbox.
        self.assertIn("model::record_fixes(mode, o, &mut rt.thinner, local, &fixes)", self.view)

    def test_never_asks_for_always_location(self):
        request = function_body(self.native, "pub(crate) fn request_permission_if_needed(")
        self.assertIn("request_permission(false)", request)
        self.assertNotIn("request_permission(true)", self.native + self.view)

    def test_jni_and_c_symbols_match(self):
        bridge = (KOTLIN / "SearchTrackBridge.kt").read_text()
        for name in ["nativeOnSearchFix", "nativeOnStopRequested"]:
            self.assertIn(f"external fun {name}(", bridge)
            self.assertIn(f"Java_com_ubseds_gs26_SearchTrackBridge_{name}", self.native)
        self.assertIn('const CLASS: &str = "com.ubseds.gs26.SearchTrackBridge";', self.native)
        shim = (ROOT / "assets/SearchTrackShim.m").read_text()
        for symbol in ["gs26_search_bg_start", "gs26_search_bg_stop", "gs26_search_bg_take"]:
            self.assertIn(symbol, shim)
            self.assertIn(f"fn {symbol}(", self.native)


class AndroidPhone(unittest.TestCase):
    def setUp(self):
        self.root = ET.parse(ROOT / "platform/android/AndroidManifest.xml").getroot()

    def test_permissions(self):
        names = {e.get(ANDROID + "name") for e in self.root.findall("uses-permission")}
        for permission in [
            "android.permission.ACCESS_FINE_LOCATION",
            "android.permission.ACCESS_COARSE_LOCATION",
            "android.permission.FOREGROUND_SERVICE",
            "android.permission.FOREGROUND_SERVICE_LOCATION",
            "android.permission.POST_NOTIFICATIONS",
        ]:
            self.assertIn(permission, names)

    def test_private_location_foreground_service(self):
        services = {s.get(ANDROID + "name"): s for s in self.root.find("application").findall("service")}
        service = services["com.ubseds.gs26.SearchTrackService"]
        self.assertEqual(service.get(ANDROID + "foregroundServiceType"), "location")
        self.assertEqual(service.get(ANDROID + "exported"), "false")

    def test_service_notification_and_stop(self):
        service = (KOTLIN / "SearchTrackService.kt").read_text()
        self.assertIn("ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION", service)
        self.assertIn(".setOngoing(true)", service)
        self.assertIn("SearchTrackPolicy.STOP_LABEL", service)
        self.assertIn("SearchTrackBridge.nativeOnStopRequested()", service)
        self.assertIn("START_NOT_STICKY", service)
        policy = (KOTLIN / "SearchTrackPolicy.kt").read_text()
        self.assertIn('STOP_LABEL = "Stop sharing"', policy)
        self.assertIn("SDK_NOTIFICATION_PERMISSION = 33", policy)

    def test_needs_only_while_in_use_location_and_sends_nothing(self):
        bridge = (KOTLIN / "SearchTrackBridge.kt").read_text()
        service = (KOTLIN / "SearchTrackService.kt").read_text()
        self.assertNotIn("ACCESS_BACKGROUND_LOCATION", bridge + service)
        self.assertIn("SearchTrackPolicy.canStart(", bridge)
        self.assertIn("Manifest.permission.POST_NOTIFICATIONS", bridge)
        for api in NETWORK_APIS:
            self.assertNotIn(api, bridge + service)

    def test_bridge_survives_minification_and_policy_is_unit_tested(self):
        rules = (ROOT / "platform/android/proguard-rules.pro").read_text()
        self.assertIn("-keep class com.ubseds.gs26.SearchTrackBridge { *; }", rules)
        gradle = (ROOT / "platform/android/unit-tests/build.gradle.kts").read_text()
        self.assertIn('include("com/ubseds/gs26/SearchTrackPolicy.kt")', gradle)
        self.assertTrue((ROOT / "platform/android/unit-tests/src/test/kotlin/com/ubseds/gs26/SearchTrackPolicyTest.kt").exists())


class IosPhone(unittest.TestCase):
    def test_shim_records_in_the_background_with_the_indicator_and_sends_nothing(self):
        shim = (ROOT / "assets/SearchTrackShim.m").read_text()
        self.assertIn("allowsBackgroundLocationUpdates = YES", shim)
        self.assertIn("showsBackgroundLocationIndicator = YES", shim)
        self.assertIn("allowsBackgroundLocationUpdates = NO", shim)
        self.assertNotIn("requestAlwaysAuthorization", shim)
        self.assertNotIn("requestWhenInUseAuthorization", shim, "Rust asks, from the user's opt-in")
        for api in NETWORK_APIS:
            self.assertNotIn(api, shim)

    def test_ios_build_compiles_the_shim(self):
        source = (ROOT / "build.rs").read_text()
        self.assertIn('assets/SearchTrackShim.m', source)
        ios_block = source[source.index("    if ios {\n        sources.push"):]
        ios_block = ios_block[:ios_block.index("    }\n")]
        self.assertIn('sources.push((search_src, out_dir.join("SearchTrackShim.o")));', ios_block)

    def test_patched_plist_has_location_mode_once_and_explains_sharing(self):
        build = load("search_platform_build", "build.py")
        ios_tests = load("search_platform_ios_tests", "tests/test_ios_bundle_platform.py")
        harness = ios_tests.IOSPlistMetadataTests("test_legacy_manual_script_defaults_to_device_labels")
        harness.setUp()
        try:
            app = harness.app(2)
            harness.script((ROOT / "scripts/patch_plist.sh").read_text())
            for _ in range(2):
                build.patch_plist(harness.root, app, ios_target="aarch64-apple-ios")
            info = plistlib.loads((app / "Info.plist").read_bytes())
            self.assertEqual(info["UIBackgroundModes"].count("location"), 1)
            when_in_use = info["NSLocationWhenInUseUsageDescription"]
            self.assertIn("recovery search", when_in_use)
            self.assertIn("only if you choose to share your track", when_in_use)
            self.assertIn("in the background", when_in_use)
            self.assertIn("stays on this phone", when_in_use)
        finally:
            harness.doCleanups()

    def test_shared_plist_says_the_same(self):
        with (ROOT / "platform/macos/Info.plist").open("rb") as f:
            info = plistlib.load(f)
        self.assertIn("only if you choose to share your track", info["NSLocationWhenInUseUsageDescription"])
        self.assertEqual(info["UIBackgroundModes"], ["location"])


class Watches(unittest.TestCase):
    def test_apple_watch_background_location_only_while_sharing(self):
        with (APPLE_WATCH / "Info.plist").open("rb") as f:
            info = plistlib.load(f)
        self.assertIn("location", info["UIBackgroundModes"])
        self.assertIn("only if you choose to share", info["NSLocationWhenInUseUsageDescription"])
        self.assertNotIn("NSLocationAlwaysAndWhenInUseUsageDescription", info)
        service = (APPLE_WATCH / "LocationService.swift").read_text()
        self.assertIn("manager.allowsBackgroundLocationUpdates = background", service)
        self.assertIn("manager.allowsBackgroundLocationUpdates = false", service)
        self.assertNotIn("requestAlwaysAuthorization", service)
        root = (APPLE_WATCH / "RootView.swift").read_text()
        update = root[root.index("private func updateLocation()"):]
        update = update[:update.index("\n    }\n")]
        self.assertEqual(update.count("background: true"), 1)
        self.assertLess(update.index("if search.recording(link) {"), update.index("background: true"))
        self.assertIn("guard search.recording(link) else {", root)
        # Background location mode, not a workout session (see the doc).
        for swift in APPLE_WATCH.glob("*.swift"):
            self.assertNotIn("HKWorkoutSession", swift.read_text(), swift.name)

    def test_wear_location_service_only_while_recording(self):
        root = ET.parse(WEAR / "AndroidManifest.xml").getroot()
        names = {e.get(ANDROID + "name") for e in root.findall("uses-permission")}
        self.assertIn("android.permission.FOREGROUND_SERVICE_LOCATION", names)
        self.assertIn("android.permission.POST_NOTIFICATIONS", names)
        self.assertNotIn("android.permission.ACCESS_BACKGROUND_LOCATION", names)
        services = {s.get(ANDROID + "name"): s for s in root.find("application").findall("service")}
        self.assertEqual(services[".SearchTrackService"].get(ANDROID + "foregroundServiceType"), "location")
        self.assertEqual(services[".SearchTrackService"].get(ANDROID + "exported"), "false")
        service = (WEAR / "kotlin/com/ubseds/gs26/wear/SearchTrackService.kt").read_text()
        start = service[service.index("override fun onStartCommand"):]
        self.assertLess(start.index("if (!link.searchRecording()"), start.index("ServiceCompat.startForeground"))
        self.assertIn("FOREGROUND_SERVICE_TYPE_LOCATION", service)
        self.assertIn(".setOngoing(true)", service)
        self.assertIn('"Stop sharing"', service)
        self.assertIn("link.shareSearch(false)", service)
        for api in NETWORK_APIS:
            self.assertNotIn(api, service)
        app = (WEAR / "kotlin/com/ubseds/gs26/wear/ui/WatchApp.kt").read_text()
        self.assertRegex(app, r"if \(searchRecording && searchPermitted\) \{\s*SearchTrackService\.start\(context\)")


class Docs(unittest.TestCase):
    def test_device_steps_are_documented(self):
        doc = (ROOT / "docs/proximity-lock.md").read_text()
        self.assertIn("## Recovery search in the background", doc)
        for platform in ["iOS", "Android", "Apple Watch", "Wear OS"]:
            self.assertIn(platform, doc[doc.index("## Recovery search in the background"):])
        watch = (ROOT / "docs/watch-companion.md").read_text()
        self.assertIn("HKWorkoutSession", watch)


if __name__ == "__main__":
    unittest.main()

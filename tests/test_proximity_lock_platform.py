"""Proximity lock platform contracts (docs/proximity-lock.md).

Android: background location permission, the location foreground service and
the geofence receiver, the Play Services location dependency. iOS: the usage
strings and the "location" background mode, added once. No SDK, device or
Apple tool is used; the iOS script runs through the test PlistBuddy adapter.
"""
import importlib.util
from pathlib import Path
import plistlib
import shlex
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


build = load("proximity_platform_build", "build.py")
ios_tests = load("proximity_platform_ios_tests", "tests/test_ios_bundle_platform.py")
ANDROID = "{http://schemas.android.com/apk/res/android}"


class AndroidManifestContract(unittest.TestCase):
    def setUp(self):
        self.root = ET.parse(ROOT / "platform/android/AndroidManifest.xml").getroot()

    def test_background_location_permissions(self):
        names = {e.get(ANDROID + "name") for e in self.root.findall("uses-permission")}
        for permission in [
            "android.permission.ACCESS_FINE_LOCATION",
            "android.permission.ACCESS_COARSE_LOCATION",
            "android.permission.ACCESS_BACKGROUND_LOCATION",
            "android.permission.FOREGROUND_SERVICE",
            "android.permission.FOREGROUND_SERVICE_LOCATION",
        ]:
            self.assertIn(permission, names)

    def test_location_service_and_private_geofence_receiver(self):
        app = self.root.find("application")
        services = {s.get(ANDROID + "name"): s for s in app.findall("service")}
        service = services["com.ubseds.gs26.ProximityService"]
        self.assertEqual(service.get(ANDROID + "foregroundServiceType"), "location")
        self.assertEqual(service.get(ANDROID + "exported"), "false")
        receivers = {r.get(ANDROID + "name"): r for r in app.findall("receiver")}
        self.assertEqual(receivers["com.ubseds.gs26.ProximityGeofenceReceiver"].get(ANDROID + "exported"), "false")

    def test_jni_bridge_survives_minification(self):
        rules = (ROOT / "platform/android/proguard-rules.pro").read_text()
        self.assertIn("-keep class com.ubseds.gs26.ProximityBridge { *; }", rules)

    def test_gradle_gets_play_services_location_once(self):
        with tempfile.TemporaryDirectory() as temp:
            project = Path(temp)
            (project / "app").mkdir()
            gradle = project / "app/build.gradle.kts"
            gradle.write_text('android {\n    compileSdk = 34\n    minSdk = 24\n    targetSdk = 34\n}\ndependencies {\n}\n')
            for _ in range(2):
                build._configure_android_app_gradle(ROOT, project)
            raw = gradle.read_text()
            self.assertEqual(raw.count('implementation("com.google.android.gms:play-services-location:21.3.0")'), 1)
            self.assertEqual(raw.count("play-services-wearable"), 1)


class ApplePlistContract(unittest.TestCase):
    def test_shared_plist_explains_background_location(self):
        with (ROOT / "platform/macos/Info.plist").open("rb") as f:
            info = plistlib.load(f)
        for key in ["NSLocationWhenInUseUsageDescription", "NSLocationAlwaysAndWhenInUseUsageDescription"]:
            self.assertIn("stays on this", info[key], key)
        self.assertIn("proximity lock", info["NSLocationAlwaysAndWhenInUseUsageDescription"])
        self.assertEqual(info["UIBackgroundModes"], ["location"])

    def test_ios_patch_adds_strings_and_location_mode_once(self):
        harness = ios_tests.IOSPlistMetadataTests("test_legacy_manual_script_defaults_to_device_labels")
        harness.setUp()
        try:
            app = harness.app(2)
            harness.script((ROOT / "scripts/patch_plist.sh").read_text())
            info = plistlib.loads((app / "Info.plist").read_bytes())
            info["UIBackgroundModes"] = ["audio"]
            (app / "Info.plist").write_bytes(plistlib.dumps(info))
            for _ in range(2):
                build.patch_plist(harness.root, app, ios_target="aarch64-apple-ios")
            info = plistlib.loads((app / "Info.plist").read_bytes())
            self.assertEqual(info["UIBackgroundModes"], ["audio", "location"])
            for key in [
                "NSLocationWhenInUseUsageDescription",
                "NSLocationAlwaysAndWhenInUseUsageDescription",
                "NSLocationAlwaysUsageDescription",
            ]:
                self.assertIn("stays on this phone", info[key], key)
        finally:
            harness.doCleanups()

    def test_ios_build_compiles_the_proximity_shim(self):
        source = (ROOT / "build.rs").read_text()
        self.assertIn('assets/ProximityShim.m', source)
        self.assertIn('sources.push((proximity_src, out_dir.join("ProximityShim.o")));', source)
        shim = (ROOT / "assets/ProximityShim.m").read_text()
        for symbol in [
            "gs26_proximity_auth_status", "gs26_proximity_request", "gs26_proximity_start",
            "gs26_proximity_stop", "gs26_proximity_take_wake", "gs26_proximity_last_fix",
            "gs26_proximity_open_settings",
        ]:
            self.assertIn(symbol, shim)
            self.assertIn(symbol, (ROOT / "src/telemetry_dashboard/proximity_native.rs").read_text())
        self.assertIn("allowsBackgroundLocationUpdates = YES", shim)
        self.assertIn("CLCircularRegion", shim)
        # The shim never sends anything: no networking API in it.
        for api in ["NSURLSession", "NSURLConnection", "dataTaskWith"]:
            self.assertNotIn(api, shim)


if __name__ == "__main__":
    unittest.main()

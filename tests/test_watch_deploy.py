"""Watch deploy actions: device discovery parsing, selection, profile checks,
command lines and error messages.

Runs anywhere: xcrun, ios-deploy, adb and Gradle are never started (the
process helpers are mocked with the tools' sample output)."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from scripts import build_targets as targets  # noqa: E402
from scripts import watch_deploy as wd  # noqa: E402

APPLE_WATCH_TOOL_PROBLEM = targets.apple_watch_tool_problem  # unpatched, for the preflight tests

spec = importlib.util.spec_from_file_location("watch_deploy_build", ROOT / "build.py")
build = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = build
spec.loader.exec_module(build)

PHONE_ID = "com.UBSEDS.GS26"
WATCH_ID = "com.UBSEDS.GS26.watchkitapp"
WATCH_CORE_ID = "8A3C1F52-6E0B-4F3D-9C2A-1B7E5D4C3B2A"
WATCH_UDID = "00008301-0012345678901234"

DEVICECTL_JSON = {
    "info": {"outcome": "success"},
    "result": {"devices": [
        {"identifier": "11111111-2222-3333-4444-555555555555",
         "connectionProperties": {"pairingState": "paired", "transportType": "wired", "tunnelState": "connected"},
         "deviceProperties": {"name": "Rylan's iPhone", "osVersionNumber": "18.1", "developerModeStatus": "enabled"},
         "hardwareProperties": {"deviceType": "iPhone", "platform": "iOS", "udid": "00008110-000A11112222801E",
                                "marketingName": "iPhone 15 Pro", "reality": "physical"}},
        {"identifier": WATCH_CORE_ID,
         "connectionProperties": {"pairingState": "paired", "tunnelState": "disconnected"},
         "deviceProperties": {"name": "Rylan's Apple Watch", "osVersionNumber": "11.1", "developerModeStatus": "enabled"},
         "hardwareProperties": {"deviceType": "appleWatch", "platform": "watchOS", "udid": WATCH_UDID,
                                "marketingName": "Apple Watch Series 9", "productType": "Watch7,3", "reality": "physical"}},
        {"identifier": "99999999-0000-0000-0000-000000000000",
         "deviceProperties": {"name": "Virtual watch"},
         "hardwareProperties": {"deviceType": "appleWatch", "platform": "watchOS", "reality": "virtual"}},
    ]},
}

DEVICECTL_TABLE = """\
Devices:
Name                  Hostname                                  Identifier                             State                Model
-------------------   ---------------------------------------   ------------------------------------   ------------------   -----------------------------
Rylan's iPhone        Rylans-iPhone.coredevice.local            11111111-2222-3333-4444-555555555555   available (paired)   iPhone 15 Pro (iPhone16,1)
Rylan's Apple Watch   Rylans-Apple-Watch.coredevice.local       8A3C1F52-6E0B-4F3D-9C2A-1B7E5D4C3B2A   available (paired)   Apple Watch Series 9 (Watch7,3)
"""

SIMCTL_PAIRS = {"pairs": {
    "PAIR-A": {"watch": {"name": "Apple Watch Series 10 (46mm)", "udid": "W-A", "state": "Shutdown"},
               "phone": {"name": "iPhone 16 Pro", "udid": "P-A", "state": "Shutdown"}, "state": "(active, disconnected)"},
    "PAIR-B": {"watch": {"name": "Apple Watch Ultra 2 (49mm)", "udid": "W-B", "state": "Shutdown"},
               "phone": {"name": "iPhone 16", "udid": "P-B", "state": "Booted"}, "state": "(inactive, disconnected)"},
}}

SIMCTL_RUNTIMES = {"runtimes": [
    {"identifier": "com.apple.CoreSimulator.SimRuntime.iOS-17-5", "name": "iOS 17.5", "version": "17.5",
     "platform": "iOS", "isAvailable": True,
     "supportedDeviceTypes": [{"identifier": "iPhone-15", "name": "iPhone 15", "productFamily": "iPhone"}]},
    {"identifier": "com.apple.CoreSimulator.SimRuntime.iOS-18-1", "name": "iOS 18.1", "version": "18.1",
     "platform": "iOS", "isAvailable": True,
     "supportedDeviceTypes": [{"identifier": "iPad-Air", "name": "iPad Air", "productFamily": "iPad"},
                              {"identifier": "iPhone-16", "name": "iPhone 16", "productFamily": "iPhone"},
                              {"identifier": "iPhone-16-Pro", "name": "iPhone 16 Pro", "productFamily": "iPhone"}]},
    {"identifier": "com.apple.CoreSimulator.SimRuntime.watchOS-11-1", "name": "watchOS 11.1", "version": "11.1",
     "platform": "watchOS", "isAvailable": True,
     "supportedDeviceTypes": [{"identifier": "Watch-S10-46", "name": "Apple Watch Series 10 (46mm)",
                               "productFamily": "Apple Watch"}]},
    {"identifier": "com.apple.CoreSimulator.SimRuntime.watchOS-12-0", "name": "watchOS 12.0", "version": "12.0",
     "platform": "watchOS", "isAvailable": False, "supportedDeviceTypes": []},
]}

ADB_DEVICES = """\
* daemon started successfully
List of devices attached
R5CT1234ABC\tdevice
192.168.1.42:41235\tdevice
emulator-5554\tdevice
emulator-5556\tdevice
"""
CHARACTERISTICS = {"R5CT1234ABC": "phone", "192.168.1.42:41235": "nosdcard,watch",
                   "emulator-5554": "emulator,watch", "emulator-5556": "emulator"}


def dev_profile(bundle, devices=("00008110-000A11112222801E", WATCH_UDID), task_allow=True):
    return {"Entitlements": {"application-identifier": f"TEAMID1234.{bundle}", "get-task-allow": task_allow},
            "ProvisionedDevices": list(devices)}


def write_plist(path, data):
    path.mkdir(parents=True, exist_ok=True)
    with (path / "Info.plist").open("wb") as f:
        plistlib.dump(data, f)


def fake_watch_app(path, bundle=WATCH_ID, companion=PHONE_ID):
    write_plist(path, {"CFBundleIdentifier": bundle, "WKCompanionAppBundleIdentifier": companion})
    return path


def commands(run_mock):
    return [[str(p) for p in c.args[0]] for c in run_mock.call_args_list]


class DevicectlParsing(unittest.TestCase):
    def test_json_lists_physical_phone_and_watch(self):
        devices = wd.parse_devicectl_json(json.dumps(DEVICECTL_JSON))
        self.assertEqual([d.kind for d in devices], ["phone", "watch"])
        watch = devices[1]
        self.assertEqual((watch.identifier, watch.udid, watch.name), (WATCH_CORE_ID, WATCH_UDID, "Rylan's Apple Watch"))
        self.assertEqual((watch.developer_mode, watch.tunnel_state), ("enabled", "disconnected"))

    def test_table_fallback(self):
        devices = wd.parse_devicectl_table(DEVICECTL_TABLE)
        self.assertEqual([(d.name, d.kind) for d in devices], [("Rylan's iPhone", "phone"), ("Rylan's Apple Watch", "watch")])
        self.assertEqual(devices[1].identifier, WATCH_CORE_ID)

    def test_bad_json_is_a_clear_error(self):
        with self.assertRaisesRegex(wd.WatchDeployError, "devicectl device list"):
            wd.parse_devicectl_json("not json")

    def test_select_single_watch_by_default_and_by_name_udid_identifier(self):
        devices = wd.parse_devicectl_json(json.dumps(DEVICECTL_JSON))
        self.assertEqual(wd.select_apple_watch(devices).identifier, WATCH_CORE_ID)
        for wanted in ["rylan's apple watch", WATCH_UDID, WATCH_CORE_ID.lower()]:
            self.assertEqual(wd.select_apple_watch(devices, wanted).identifier, WATCH_CORE_ID)

    def test_no_watch(self):
        phone_only = [d for d in wd.parse_devicectl_json(json.dumps(DEVICECTL_JSON)) if d.kind == "phone"]
        with self.assertRaises(wd.WatchDeployError) as ctx:
            wd.select_apple_watch(phone_only)
        self.assertIn("No paired Apple Watch", str(ctx.exception))
        self.assertIn("watch=skip", str(ctx.exception))

    def test_several_watches_need_a_choice(self):
        a = wd.AppleDevice("A", "Work Watch", "watch")
        b = wd.AppleDevice("B", "Home Watch", "watch")
        with self.assertRaisesRegex(wd.WatchDeployError, "watch=<name"):
            wd.select_apple_watch([a, b])
        self.assertEqual(wd.select_apple_watch([a, b], "home watch").identifier, "B")
        with self.assertRaisesRegex(wd.WatchDeployError, "No Apple Watch matches watch=Other"):
            wd.select_apple_watch([a, b], "Other")

    def test_wanted_device_that_is_not_a_watch(self):
        devices = wd.parse_devicectl_json(json.dumps(DEVICECTL_JSON))
        with self.assertRaisesRegex(wd.WatchDeployError, "not an Apple Watch"):
            wd.select_apple_watch(devices, "Rylan's iPhone")

    def test_developer_mode_off(self):
        watch = wd.AppleDevice("A", "Rylan's Apple Watch", "watch", developer_mode="disabled")
        with self.assertRaisesRegex(wd.WatchDeployError, "Developer Mode is off on Rylan's Apple Watch"):
            wd.select_apple_watch([watch])

    def test_command_lines(self):
        self.assertEqual(wd.devicectl_list_command("/tmp/d.json"),
                         ["xcrun", "devicectl", "list", "devices", "--json-output", "/tmp/d.json"])
        self.assertEqual(wd.devicectl_install_command("ID", Path("/a/GS26.app/Watch/GS26Watch.app")),
                         ["xcrun", "devicectl", "device", "install", "app", "--device", "ID", "/a/GS26.app/Watch/GS26Watch.app"])
        self.assertFalse(hasattr(wd, "devicectl_launch_command"))  # install only, like ios_deploy

    def test_unreachable_note_and_manual_instructions(self):
        for state in ["connected", "disconnected", ""]:
            self.assertIsNone(wd.watch_unreachable_note(wd.AppleDevice("A", "W", "watch", tunnel_state=state)))
        self.assertIn("charger", wd.watch_unreachable_note(wd.AppleDevice("A", "W", "watch", tunnel_state="unavailable")))
        text = wd.watch_manual_install_instructions(PHONE_ID)
        self.assertIn("Available Apps", text)
        self.assertIn("Automatic App Install", text)

    def test_install_failure_help(self):
        idle = wd.AppleDevice("A", "Rylan's Apple Watch", "watch", tunnel_state="disconnected")
        text = wd.watch_install_failure_help(idle)
        for needle in ["Rylan's Apple Watch", "unlocked", "charger", "Developer Mode", "Devices and",
                       "watchos_deploy with_ios", "Watch app"]:
            self.assertIn(needle, text)
        self.assertNotIn("unavailable to Xcode", text)
        self.assertIn("unavailable to Xcode", wd.watch_install_failure_help(
            wd.AppleDevice("A", "W", "watch", tunnel_state="unavailable")))


class BundleAndProfileChecks(unittest.TestCase):
    def test_watch_bundle_id_must_be_phone_id_plus_watchkitapp(self):
        with tempfile.TemporaryDirectory() as d:
            ok = fake_watch_app(Path(d) / "ok.app")
            self.assertEqual(wd.check_watch_app_bundle(ok, PHONE_ID), WATCH_ID)
            wrong = fake_watch_app(Path(d) / "wrong.app", bundle="com.UBSEDS.GS26.watch")
            with self.assertRaisesRegex(wd.WatchDeployError, r"'com\.UBSEDS\.GS26\.watch', expected 'com\.UBSEDS\.GS26\.watchkitapp'"):
                wd.check_watch_app_bundle(wrong, PHONE_ID)
            companion = fake_watch_app(Path(d) / "companion.app", companion="com.example.Other")
            with self.assertRaisesRegex(wd.WatchDeployError, "WKCompanionAppBundleIdentifier"):
                wd.check_watch_app_bundle(companion, PHONE_ID)

    def test_development_profile_problems(self):
        match = build._profile_matches_bundle
        self.assertIsNone(wd.development_profile_problem(dev_profile(WATCH_ID), WATCH_ID, match, WATCH_UDID))
        self.assertIsNone(wd.development_profile_problem(dev_profile("com.UBSEDS.*"), WATCH_ID, match))
        self.assertIn("not a development profile",
                      wd.development_profile_problem(dev_profile(WATCH_ID, task_allow=False), WATCH_ID, match))
        self.assertIn(f"not {WATCH_ID}", wd.development_profile_problem(dev_profile(PHONE_ID), WATCH_ID, match))
        self.assertIn("no ProvisionedDevices", wd.development_profile_problem(dev_profile(WATCH_ID, devices=()), WATCH_ID, match))
        self.assertIn("does not list device 0000-OTHER",
                      wd.development_profile_problem(dev_profile(WATCH_ID), WATCH_ID, match, "0000-OTHER"))

    def test_watch_development_profile_lookup_mirrors_the_ios_one(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            profile = root / "elsewhere.mobileprovision"
            profile.write_bytes(b"x")
            with patch.dict(os.environ, {"IOS_WATCH_DEVELOPMENT_MOBILEPROVISION": str(profile)}):
                self.assertEqual(build.ios_watch_mobileprovision_path("development", root, bundle_id=WATCH_ID), profile)
            with patch.dict(os.environ, {"IOS_WATCH_DEVELOPMENT_MOBILEPROVISION": str(root / "missing")}):
                with self.assertRaisesRegex(FileNotFoundError, "IOS_WATCH_DEVELOPMENT_MOBILEPROVISION does not exist"):
                    build.ios_watch_mobileprovision_path("development", root, bundle_id=WATCH_ID)
            with patch.dict(os.environ, {"IOS_WATCH_DEVELOPMENT_MOBILEPROVISION": ""}), \
                 patch.object(build, "_find_installed_ios_development_profile", return_value=None) as installed:
                with self.assertRaisesRegex(RuntimeError, "Missing Apple Watch development provisioning profile") as ctx:
                    build.ios_watch_mobileprovision_path("development", root, bundle_id=WATCH_ID)
                self.assertIn("Groundstation_watch_dev.mobileprovision", str(ctx.exception))
                installed.assert_called_once_with(root, WATCH_ID)
                local = root / "Groundstation_watch_dev.mobileprovision"
                local.write_bytes(b"x")
                self.assertEqual(build.ios_watch_mobileprovision_path("development", root, bundle_id=WATCH_ID), local)
            with patch.dict(os.environ, {"IOS_WATCH_DISTRIBUTION_MOBILEPROVISION": ""}):
                with self.assertRaisesRegex(RuntimeError, "IOS_WATCH_DISTRIBUTION_MOBILEPROVISION"):
                    build.ios_watch_mobileprovision_path("distribution", root)


class WatchOSDeviceDeploy(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.product = fake_watch_app(targets.watchos_product(self.root, "watchos", False))
        self.app = self.root / "dist" / build.APP_BUNDLE_NAME
        self.profiles = {"phone.mobileprovision": dev_profile(PHONE_ID), "watch.mobileprovision": dev_profile(WATCH_ID)}
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        self.addCleanup(self.tmp.cleanup)
        p = lambda *a, **k: self.stack.enter_context(patch.object(*a, **k))
        p(build.platform, "system", return_value="Darwin")
        self.preflight = p(targets, "apple_watch_tool_problem", return_value=None)
        p(build, "_bundle_identifier", return_value=PHONE_ID)
        self.devices = p(build, "_devicectl_devices", return_value=wd.parse_devicectl_json(json.dumps(DEVICECTL_JSON)))
        p(build, "ios_development_mobileprovision_path", return_value=Path("phone.mobileprovision"))
        p(build, "ios_watch_mobileprovision_path", return_value=Path("watch.mobileprovision"))
        p(build, "_decode_mobileprovision", side_effect=lambda path, _root: self.profiles[path.name])
        self.build_frontend = p(build, "build_frontend")
        self.package = p(build, "package_ios_ipa_with_script", side_effect=self._sign)
        p(build, "_select_ios_device_udids", return_value=["00008110-000A11112222801E"])
        self.run = p(build, "run")
        self.out = self.stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
        self.err = self.stack.enter_context(contextlib.redirect_stderr(io.StringIO()))
        self.signed = self.root / "target/watch/watchos-signed" / targets.WATCH_APP_NAME
        (self.root / "scripts").mkdir()
        (self.root / "scripts/ios_package_sign.sh").write_text("")  # never run: `run` is mocked

    def _sign(self, root, *, sign_kind, debug_mode, watch_app):
        self.assertEqual(sign_kind, "development")
        fake_watch_app(self.app / "Watch" / targets.WATCH_APP_NAME)
        return root / "dist/ipas/UBSEDS GS Development.ipa"

    # Default: only the watch app is built, signed and installed.

    def test_default_builds_signs_and_installs_only_the_watch_app(self):
        udids, watch, bundle = build.watchos_device_deploy(self.root)
        self.assertEqual((udids, watch, bundle), ([], "Rylan's Apple Watch", PHONE_ID))
        self.build_frontend.assert_not_called()
        self.package.assert_not_called()
        build._select_ios_device_udids.assert_not_called()
        cmds = commands(self.run)
        self.assertEqual([c[0] for c in cmds[:2]], ["xcodegen", "xcodebuild"])
        self.assertIn("-sdk", cmds[1])
        self.assertEqual(cmds[1][cmds[1].index("-sdk") + 1], "watchos")
        # The watch app is signed on its own with the watch profile (dev, get-task-allow).
        self.assertEqual(cmds[2][:4], ["bash", str(self.root / "scripts/ios_package_sign.sh"),
                                       str(self.signed.resolve()), str(Path("watch.mobileprovision").resolve())])
        self.assertEqual(self.run.call_args_list[2].kwargs["env"]["GET_TASK_ALLOW"], "true")
        self.assertTrue((self.signed / "Info.plist").is_file())
        self.assertEqual(cmds[3], wd.devicectl_install_command(WATCH_CORE_ID, self.signed))
        self.assertEqual(len(cmds), 4)
        joined = " ".join(" ".join(c) for c in cmds)
        self.assertNotIn("ios-deploy", joined)
        self.assertNotIn("dx", [c[0] for c in cmds])
        self.assertFalse(any("launch" in c for c in cmds))
        self.preflight.assert_called_once_with(xcodegen=True, ios_deploy=False)
        self.assertIn("open GS26 from the app list on Rylan's Apple Watch", self.out.getvalue())

    def test_default_checks_only_the_watch_profile(self):
        build.watchos_device_deploy(self.root, use_existing=True)
        build.ios_development_mobileprovision_path.assert_not_called()
        build.ios_watch_mobileprovision_path.assert_called_once_with("development", self.root, bundle_id=WATCH_ID)
        self.profiles["phone.mobileprovision"] = dev_profile(PHONE_ID, task_allow=False)  # irrelevant by default
        build.watchos_device_deploy(self.root, use_existing=True)
        self.run.reset_mock()
        self.profiles["watch.mobileprovision"] = dev_profile(WATCH_ID, devices=("OTHER",))
        with self.assertRaisesRegex(wd.WatchDeployError, f"Apple Watch .* does not list device {WATCH_UDID}"):
            build.watchos_device_deploy(self.root)
        self.run.assert_not_called()  # checked before building

    def test_default_preflight_does_not_need_ios_deploy(self):
        build.watchos_device_deploy(self.root, use_existing=True)
        self.preflight.assert_called_once_with(xcodegen=False, ios_deploy=False)
        # Without ios-deploy installed, the watch-only preflight passes; with_ios still asks for it.
        which = {"xcodegen": "/b/xcodegen", "xcrun": "/b/xcrun", "xcodebuild": "/b/xcodebuild"}.get
        self.assertIsNone(APPLE_WATCH_TOOL_PROBLEM(ios_deploy=False, which=which, xcode_ok=lambda: True))
        self.assertIn("brew install ios-deploy",
                      APPLE_WATCH_TOOL_PROBLEM(ios_deploy=True, which=which, xcode_ok=lambda: True))

    def test_default_companion_id_is_still_checked(self):
        fake_watch_app(self.product, companion="com.other.app")
        with self.assertRaisesRegex(wd.WatchDeployError, "WKCompanionAppBundleIdentifier"):
            build.watchos_device_deploy(self.root, use_existing=True)
        self.run.assert_not_called()

    def test_default_install_failure_explains_and_suggests_with_ios(self):
        self.run.side_effect = lambda cmd, **_: (_ for _ in ()).throw(subprocess.CalledProcessError(1, cmd)) \
            if cmd[:3] == ["xcrun", "devicectl", "device"] else None
        with self.assertRaises(wd.WatchDeployError) as ctx:
            build.watchos_device_deploy(self.root, use_existing=True)
        text = str(ctx.exception)
        for needle in ["unlocked", "charger", "Developer Mode", "Devices and", "watchos_deploy with_ios"]:
            self.assertIn(needle, text)
        self.assertNotIn("ios-deploy", " ".join(" ".join(c) for c in commands(self.run)))

    def test_default_rejects_phone_only_options(self):
        with self.assertRaisesRegex(wd.WatchDeployError, "with_ios"):
            build.watchos_device_deploy(self.root, watch_wanted="skip")
        with self.assertRaisesRegex(wd.WatchDeployError, "with_ios"):
            build.watchos_device_deploy(self.root, phone_wanted="PHONE-UDID")
        self.run.assert_not_called()

    # with_ios: the old phone + watch behaviour.

    def test_builds_signs_installs_phone_then_watch_without_launching(self):
        udids, watch, bundle = build.watchos_device_deploy(self.root, with_ios=True)
        self.assertEqual((udids, watch, bundle), (["00008110-000A11112222801E"], "Rylan's Apple Watch", PHONE_ID))
        self.build_frontend.assert_called_once_with(self.root, platform_name="ios", rust_target="aarch64-apple-ios",
                                                    debug_mode=False, max_size=False)
        self.package.assert_called_once_with(self.root, sign_kind="development", debug_mode=False, watch_app=self.product)
        cmds = commands(self.run)
        self.assertEqual([c[0] for c in cmds[:2]], ["xcodegen", "xcodebuild"])
        self.assertIn("ARCHS=arm64_32 arm64", cmds[1])
        self.assertEqual(cmds[2], ["ios-deploy", "--id", "00008110-000A11112222801E", "--bundle", str(self.app)])
        self.assertEqual(cmds[3], wd.devicectl_install_command(WATCH_CORE_ID, self.app / "Watch" / "GS26Watch.app"))
        # Like ios_deploy (ios-deploy --bundle, no --justlaunch/--debug): install only.
        self.assertEqual(len(cmds), 4)
        self.assertFalse(any("launch" in c for c in cmds))
        self.preflight.assert_called_once_with(xcodegen=True, ios_deploy=True)
        self.assertIn("open GS26 from the app list on Rylan's Apple Watch", self.out.getvalue())

    def test_missing_tools_stop_before_any_device_or_build_work(self):
        self.preflight.return_value = "xcodegen and ios-deploy not found: brew install xcodegen ios-deploy"
        with self.assertRaisesRegex(wd.WatchDeployError, "(?s)watchos_deploy needs tools.*brew install xcodegen ios-deploy"):
            build.watchos_device_deploy(self.root, with_ios=True)
        self.devices.assert_not_called()
        self.build_frontend.assert_not_called()
        self.run.assert_not_called()

    def test_existing_does_not_need_xcodegen(self):
        build.watchos_device_deploy(self.root, use_existing=True, with_ios=True)
        self.preflight.assert_called_once_with(xcodegen=False, ios_deploy=True)

    def test_existing_reuses_builds_and_phone_flag_selects_the_iphone(self):
        build.watchos_device_deploy(self.root, use_existing=True, phone_wanted="PHONE-UDID", watch_wanted="Rylan's Apple Watch",
                                    with_ios=True)
        self.build_frontend.assert_not_called()
        cmds = commands(self.run)
        self.assertEqual(cmds[0], ["ios-deploy", "--id", "PHONE-UDID", "--bundle", str(self.app)])
        build._select_ios_device_udids.assert_not_called()

    def test_watch_install_failure_falls_back_to_instructions(self):
        self.run.side_effect = lambda cmd, **_: (_ for _ in ()).throw(subprocess.CalledProcessError(1, cmd)) \
            if cmd[:3] == ["xcrun", "devicectl", "device"] else None
        _udids, watch, _bundle = build.watchos_device_deploy(self.root, use_existing=True, with_ios=True)
        self.assertIsNone(watch)
        self.assertIn("Available Apps", self.out.getvalue())

    def test_watch_skip_installs_through_the_iphone(self):
        _udids, watch, _bundle = build.watchos_device_deploy(self.root, use_existing=True, watch_wanted="skip", with_ios=True)
        self.assertIsNone(watch)
        self.devices.assert_not_called()
        self.assertNotIn("devicectl", " ".join(" ".join(c) for c in commands(self.run)))
        self.assertIn("Available Apps", self.out.getvalue())

    def test_profiles_are_checked_before_building(self):
        cases = {
            "phone.mobileprovision": (dev_profile(PHONE_ID, task_allow=False), "iOS development provisioning profile .* not a development profile"),
            "watch.mobileprovision": (dev_profile(WATCH_ID, devices=("OTHER",)), f"Apple Watch .* does not list device {WATCH_UDID}"),
        }
        for name, (info, message) in cases.items():
            with self.subTest(name=name):
                saved = self.profiles[name]
                self.profiles[name] = info
                with self.assertRaisesRegex(wd.WatchDeployError, message):
                    build.watchos_device_deploy(self.root, with_ios=True)
                self.profiles[name] = saved
        self.build_frontend.assert_not_called()
        self.run.assert_not_called()

    def test_missing_watch_profile(self):
        build.ios_watch_mobileprovision_path.side_effect = RuntimeError("Missing Apple Watch development provisioning profile.")
        with self.assertRaisesRegex(wd.WatchDeployError, "Missing Apple Watch development provisioning profile"):
            build.watchos_device_deploy(self.root)

    def test_bundle_id_mismatch_stops_before_signing(self):
        fake_watch_app(self.product, bundle="com.UBSEDS.GS26.watch")
        with self.assertRaisesRegex(wd.WatchDeployError, "expected 'com.UBSEDS.GS26.watchkitapp'"):
            build.watchos_device_deploy(self.root, use_existing=True, with_ios=True)
        self.package.assert_not_called()

    def test_devicectl_listing_is_read_from_the_json_file(self):
        self.stack.close()
        self.stack = contextlib.ExitStack()

        def fake_capture(cmd, cwd, env=None):
            Path(cmd[cmd.index("--json-output") + 1]).write_text(json.dumps(DEVICECTL_JSON))
            return ""
        with patch.object(build, "run_capture", side_effect=fake_capture) as capture, quiet():
            devices = build._devicectl_devices(self.root)
        self.assertEqual(capture.call_args.args[0][:4], ["xcrun", "devicectl", "list", "devices"])
        self.assertEqual([d.kind for d in devices], ["phone", "watch"])
        with patch.object(build, "run_capture", side_effect=subprocess.CalledProcessError(64, ["xcrun"])), quiet():
            with self.assertRaisesRegex(wd.WatchDeployError, "Xcode 15"):
                build._devicectl_devices(self.root)


def quiet():
    return contextlib.redirect_stdout(io.StringIO())


class SimctlParsing(unittest.TestCase):
    def test_pairs_prefer_booted_then_active(self):
        pairs = wd.parse_simctl_pairs(json.dumps(SIMCTL_PAIRS))
        self.assertEqual({p.pair_id for p in pairs}, {"PAIR-A", "PAIR-B"})
        self.assertEqual(wd.choose_sim_pair(pairs).pair_id, "PAIR-B")
        shut = [p for p in pairs if p.pair_id == "PAIR-A"]
        self.assertTrue(shut[0].active)
        self.assertFalse([p for p in pairs if p.pair_id == "PAIR-B"][0].active)
        self.assertIsNone(wd.choose_sim_pair(wd.parse_simctl_pairs('{"pairs": {}}')))

    def test_runtime_and_device_type_choice(self):
        text = json.dumps(SIMCTL_RUNTIMES)
        ios, watch = wd.choose_runtime(text, "iOS"), wd.choose_runtime(text, "watchOS")
        self.assertEqual(ios["identifier"], "com.apple.CoreSimulator.SimRuntime.iOS-18-1")
        self.assertEqual(watch["identifier"], "com.apple.CoreSimulator.SimRuntime.watchOS-11-1")  # 12.0 unavailable
        self.assertEqual(wd.choose_device_type(ios, "iPhone")["identifier"], "iPhone-16-Pro")
        self.assertEqual(wd.choose_device_type(watch, "Apple Watch")["identifier"], "Watch-S10-46")
        with self.assertRaisesRegex(wd.WatchDeployError, "No watchOS simulator runtime"):
            wd.choose_runtime('{"runtimes": []}', "watchOS")

    def test_host_target(self):
        self.assertEqual(wd.simulator_target_name("arm64"), "watchos_sim")
        self.assertEqual(wd.simulator_target_name("x86_64"), "watchos_sim_intel")


class WatchOSSimulatorDeploy(unittest.TestCase):
    def _deploy(self, machine, pairs_json, with_ios=False):
        with tempfile.TemporaryDirectory() as d, quiet() as out:
            root = Path(d)
            name = wd.simulator_target_name(machine)
            product = fake_watch_app(targets.watchos_product(root, name, True))
            app = root / "dist" / build.APP_BUNDLE_NAME
            write_plist(app, {"CFBundleIdentifier": PHONE_ID})
            outputs = {("list", "pairs"): pairs_json, ("list", "runtimes"): json.dumps(SIMCTL_RUNTIMES)}
            created = iter(["NEW-PHONE\n", "NEW-WATCH\n"])

            def capture(cmd, cwd, env=None):
                if cmd[2] == "create": return next(created)
                if cmd[2] == "pair": return "NEW-PAIR\n"
                return outputs[tuple(cmd[2:4])]
            tools = Mock()
            with patch.object(build.platform, "system", return_value="Darwin"), \
                 patch.object(targets, "apple_watch_tool_problem", return_value=None) as preflight, \
                 patch.object(build.platform, "machine", return_value=machine), \
                 patch.object(build, "_bundle_identifier", return_value=PHONE_ID), \
                 patch.object(build, "build_frontend") as build_frontend, \
                 patch.object(build, "_stage_app_bundle_from_dx", return_value=app) as stage, \
                 patch.object(build, "_ios_bundle_tools", return_value=tools), \
                 patch.object(build, "patch_plist"), \
                 patch.object(build, "run_capture", side_effect=capture) as run_capture, \
                 patch.object(build, "run") as run:
                result = build.watchos_simulator_deploy(root, debug_mode=True, with_ios=with_ios)
            preflight.assert_called_once_with(xcodegen=True)
            if with_ios:
                build_frontend.assert_called_once_with(root, platform_name="ios", rust_target=targets.TARGETS[name].triple,
                                                       debug_mode=True, max_size=False)
                tools.validate_app.assert_called_once_with(app, targets.TARGETS[name].triple)
                self.assertTrue((app / "Watch/GS26Watch.app/Info.plist").is_file())
            else:
                build_frontend.assert_not_called()
                stage.assert_not_called()
                self.assertFalse((app / "Watch").exists())
            return result, commands(run), [c.args[0] for c in run_capture.call_args_list], app, product

    def test_default_installs_only_the_watch_app_with_the_phone_booted(self):
        (phone, watch, bundle), cmds, captured, _app, product = self._deploy("arm64", '{"pairs": {}}')
        self.assertEqual((phone, watch, bundle), ("NEW-PHONE", "NEW-WATCH", PHONE_ID))
        self.assertIn(["xcrun", "simctl", "pair", "NEW-WATCH", "NEW-PHONE"], captured)
        self.assertEqual([c[0] for c in cmds[:2]], ["xcodegen", "xcodebuild"])
        self.assertEqual(cmds[2:], [
            ["codesign", "--force", "--sign", "-", "--timestamp=none", str(product)],
            ["xcrun", "simctl", "boot", "NEW-PHONE"], ["xcrun", "simctl", "bootstatus", "NEW-PHONE", "-b"],
            ["xcrun", "simctl", "boot", "NEW-WATCH"], ["xcrun", "simctl", "bootstatus", "NEW-WATCH", "-b"],
            ["open", "-a", "Simulator"],
            ["xcrun", "simctl", "install", "NEW-WATCH", str(product)],
            ["xcrun", "simctl", "launch", "NEW-WATCH", WATCH_ID],
        ])

    def test_with_ios_creates_pair_when_none_exists_and_installs_both(self):
        (phone, watch, bundle), cmds, captured, app, _product = self._deploy("arm64", '{"pairs": {}}', with_ios=True)
        self.assertEqual((phone, watch, bundle), ("NEW-PHONE", "NEW-WATCH", PHONE_ID))
        self.assertIn(["xcrun", "simctl", "create", "GS26 iPhone", "iPhone-16-Pro", "com.apple.CoreSimulator.SimRuntime.iOS-18-1"], captured)
        self.assertIn(["xcrun", "simctl", "create", "GS26 Apple Watch", "Watch-S10-46", "com.apple.CoreSimulator.SimRuntime.watchOS-11-1"], captured)
        self.assertIn(["xcrun", "simctl", "pair", "NEW-WATCH", "NEW-PHONE"], captured)
        self.assertIn("ARCHS=arm64", cmds[1])
        embedded = str(app / "Watch/GS26Watch.app")
        self.assertEqual(cmds[2:], [
            ["codesign", "--force", "--sign", "-", "--timestamp=none", embedded],
            ["codesign", "--force", "--sign", "-", "--timestamp=none", str(app)],
            ["xcrun", "simctl", "boot", "NEW-PHONE"], ["xcrun", "simctl", "bootstatus", "NEW-PHONE", "-b"],
            ["xcrun", "simctl", "boot", "NEW-WATCH"], ["xcrun", "simctl", "bootstatus", "NEW-WATCH", "-b"],
            ["open", "-a", "Simulator"],
            ["xcrun", "simctl", "install", "NEW-PHONE", str(app)],
            ["xcrun", "simctl", "install", "NEW-WATCH", embedded],
            ["xcrun", "simctl", "launch", "NEW-PHONE", PHONE_ID],
            ["xcrun", "simctl", "launch", "NEW-WATCH", WATCH_ID],
        ])
        self.assertNotIn("pair_activate", [c[2] for c in cmds if c[:2] == ["xcrun", "simctl"]])

    def test_missing_tools_stop_the_simulator_deploy_before_building(self):
        with patch.object(build.platform, "system", return_value="Darwin"), \
             patch.object(targets, "apple_watch_tool_problem", return_value="xcodegen not found: brew install xcodegen"), \
             patch.object(build, "build_frontend") as build_frontend, patch.object(build, "run") as run, \
             patch.object(build, "run_capture") as run_capture, quiet():
            with self.assertRaisesRegex(wd.WatchDeployError, "(?s)watchos_sim_deploy needs tools.*brew install xcodegen"):
                build.watchos_simulator_deploy(Path("/nonexistent"))
        build_frontend.assert_not_called()
        run.assert_not_called()
        run_capture.assert_not_called()

    def test_existing_inactive_pair_is_activated_and_booted_phone_reused_on_intel(self):
        for with_ios in (False, True):
            with self.subTest(with_ios=with_ios):
                (phone, watch, _), cmds, _captured, _app, _product = self._deploy(
                    "x86_64", json.dumps(SIMCTL_PAIRS), with_ios=with_ios)
                self.assertEqual((phone, watch), ("P-B", "W-B"))
                self.assertIn("ARCHS=x86_64", cmds[1])
                self.assertIn(["xcrun", "simctl", "pair_activate", "PAIR-B"], cmds)
                self.assertNotIn(["xcrun", "simctl", "boot", "P-B"], cmds)
                self.assertIn(["xcrun", "simctl", "boot", "W-B"], cmds)
                self.assertEqual(any(c[:4] == ["xcrun", "simctl", "install", "P-B"] for c in cmds), with_ios)


class AdbSelection(unittest.TestCase):
    def setUp(self):
        self.rows = wd.parse_adb_devices(ADB_DEVICES)

    def test_parse(self):
        self.assertEqual(self.rows[0], ("R5CT1234ABC", "device"))
        self.assertEqual(len(self.rows), 4)
        self.assertTrue(wd.is_watch_characteristics("nosdcard,watch"))
        self.assertFalse(wd.is_watch_characteristics("phone,smartwatchless"))

    def test_physical_watch_skips_phone_and_emulators(self):
        self.assertEqual(wd.select_wear_serial(self.rows, CHARACTERISTICS, emulator=False), "192.168.1.42:41235")

    def test_emulator_picks_the_wear_emulator(self):
        self.assertEqual(wd.select_wear_serial(self.rows, CHARACTERISTICS, emulator=True), "emulator-5554")

    def test_several_need_serial(self):
        chars = dict(CHARACTERISTICS, **{"emulator-5556": "emulator,watch"})
        with self.assertRaisesRegex(wd.WatchDeployError, "serial=<serial>"):
            wd.select_wear_serial(self.rows, chars, emulator=True)
        self.assertEqual(wd.select_wear_serial(self.rows, chars, emulator=True, wanted="emulator-5556"), "emulator-5556")

    def test_wanted_serial_errors(self):
        with self.assertRaisesRegex(wd.WatchDeployError, "not a Wear OS device"):
            wd.select_wear_serial(self.rows, CHARACTERISTICS, emulator=False, wanted="R5CT1234ABC")
        with self.assertRaisesRegex(wd.WatchDeployError, "adb does not list serial=nope"):
            wd.select_wear_serial(self.rows, CHARACTERISTICS, emulator=False, wanted="nope")

    def test_none_found_explains_wifi_pairing(self):
        with self.assertRaises(wd.WatchDeployError) as ctx:
            wd.select_wear_serial([("R5CT1234ABC", "device")], CHARACTERISTICS, emulator=False)
        self.assertIn("adb pair <ip>:<pairing-port>", str(ctx.exception))
        self.assertIn("adb connect <ip>:<port>", str(ctx.exception))
        with self.assertRaisesRegex(wd.WatchDeployError, "unauthorized"):
            wd.select_wear_serial([("10.0.0.5:5555", "unauthorized")], {}, emulator=False)
        with self.assertRaisesRegex(wd.WatchDeployError, "Start a Wear OS AVD"):
            wd.select_wear_serial([], {}, emulator=True)

    def test_wear_avds_from_config(self):
        with tempfile.TemporaryDirectory() as d:
            for name, config in {"Wear_OS_Large_Round_API_34": "tag.id=android-wear\nabi.type=x86_64\n",
                                 "Pixel_8_API_35": "tag.id=google_apis_playstore\n"}.items():
                (Path(d) / f"{name}.avd").mkdir()
                (Path(d) / f"{name}.avd" / "config.ini").write_text(config)
            self.assertEqual(wd.wear_avd_names(["Pixel_8_API_35", "Wear_OS_Large_Round_API_34", "Gone"], d),
                             ["Wear_OS_Large_Round_API_34"])

    def test_no_launch_helper(self):
        self.assertFalse(hasattr(wd, "wear_launch_command"))  # install only, like android_install


class WearOSDeploy(unittest.TestCase):
    def _deploy(self, *, simulator, existing=True, debug=False, adb_text=ADB_DEVICES, **kwargs):
        def capture(cmd, cwd, env=None):
            if cmd[1:] == ["devices"]: return adb_text
            if cmd[4:] == ["getprop", "ro.build.characteristics"]: return CHARACTERISTICS.get(cmd[2], "") + "\n"
            raise AssertionError(cmd)
        env = {"PATH": "/sdk/platform-tools", "ANDROID_HOME": "/sdk"}
        with patch.object(build, "_ensure_android_env", return_value=env), \
             patch.object(targets, "wear_tool_problem", return_value=self.tool_problem) as self.preflight, \
             patch.object(build, "_resolve_adb", return_value="adb"), \
             patch.object(build, "run_capture", side_effect=capture), \
             patch.object(build, "_wait_for_android_package_manager") as wait, \
             patch.object(build, "_install_android_apk_with_fallbacks") as install, \
             patch.object(build, "run") as run, quiet():
            result = build.wearos_device_deploy(self.root, simulator=simulator, use_existing=existing,
                                                debug_mode=debug, **kwargs)
        return result, wait, install, run

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.apk = self.root / "dist" / targets.WEAR_APK_NAME
        self.apk.parent.mkdir()
        self.apk.write_bytes(b"apk")
        self.tool_problem = None

    def test_physical_watch_install_without_launch(self):
        (serial, apk), wait, install, run = self._deploy(simulator=False)
        self.assertEqual((serial, apk), ("192.168.1.42:41235", self.apk))
        env = wait.call_args.args[2]
        wait.assert_called_once_with(self.root, serial, env)
        install.assert_called_once_with(self.root, serial, self.apk, env)
        run.assert_not_called()  # install only, like android_install
        self.preflight.assert_called_once_with(Path("/sdk"), adb=True, path="/sdk/platform-tools")

    def test_missing_adb_stops_before_device_discovery(self):
        self.tool_problem = "adb not found: install the SDK platform-tools"
        with patch.object(targets, "build_watch") as build_watch:
            with self.assertRaisesRegex(wd.WatchDeployError, "(?s)wearos_sim_deploy needs tools.*platform-tools"):
                self._deploy(simulator=True, existing=False)
        build_watch.assert_not_called()

    def test_emulator_with_serial_flag(self):
        (serial, _apk), *_ = self._deploy(simulator=True, serial_wanted="emulator-5554")
        self.assertEqual(serial, "emulator-5554")

    def test_no_emulator_lists_wear_avds(self):
        with patch.object(build, "_wear_avd_hint", return_value="\n  Wear OS AVDs: Wear_OS_Small"):
            with self.assertRaisesRegex(wd.WatchDeployError, "(?s)No Wear OS emulator.*Wear_OS_Small"):
                self._deploy(simulator=True, adb_text="List of devices attached\nR5CT1234ABC\tdevice\n")

    def test_missing_existing_apk(self):
        self.apk.unlink()
        with self.assertRaisesRegex(wd.WatchDeployError, "No Wear OS APK"):
            self._deploy(simulator=False)

    def test_build_uses_the_target_builder_and_refuses_an_unsigned_release(self):
        def fake_build(root, name, args):
            self.assertEqual((name, args.debug, args.dry_run, args.no_sign), ("wearos", False, False, False))
            unsigned = targets.wearos_product(root, False)
            unsigned.parent.mkdir(parents=True, exist_ok=True)
            unsigned.write_bytes(b"apk")
            return self.apk
        with patch.object(targets, "build_watch", side_effect=fake_build), patch.dict(os.environ, {}):
            with self.assertRaisesRegex(wd.WatchDeployError, "unsigned"):
                self._deploy(simulator=False, existing=False)

    def test_debug_build_installs(self):
        with patch.object(targets, "build_watch", return_value=self.apk) as build_watch, patch.dict(os.environ, {}):
            (serial, _apk), _wait, install, _run = self._deploy(simulator=True, existing=False, debug=True)
        self.assertEqual(build_watch.call_args.args[1], "wearos_sim")
        self.assertTrue(build_watch.call_args.args[2].debug)
        self.assertEqual(install.call_args.args[1], serial)


class Dispatch(unittest.TestCase):
    def _main(self, *argv):
        with patch.object(sys, "argv", ["build.py", *argv]), \
             patch.object(build, "_configure_log_file"), quiet():
            build.main()

    def test_actions_and_flags_reach_the_deploy_functions(self):
        with patch.object(build, "wearos_device_deploy", return_value=("emulator-5554", Path("x.apk"))) as wear:
            self._main("wearos_sim_deploy", "debug", "existing", "serial=emulator-5554")
        wear.assert_called_once_with(build._resolve_frontend_dir(), simulator=True, debug_mode=True,
                                     use_existing=True, no_sign=False, serial_wanted="emulator-5554")
        with patch.object(build, "watchos_device_deploy", return_value=(["P"], "W", PHONE_ID)) as watch:
            self._main("watchos_deploy", "watch=Rylan's Apple Watch", "phone=ABC")
        self.assertEqual(watch.call_args.kwargs["watch_wanted"], "Rylan's Apple Watch")
        self.assertEqual(watch.call_args.kwargs["phone_wanted"], "ABC")
        self.assertFalse(watch.call_args.kwargs["with_ios"])
        with patch.object(build, "watchos_device_deploy", return_value=(["P"], "W", PHONE_ID)) as watch:
            self._main("watchos_deploy", "with_ios")
        self.assertTrue(watch.call_args.kwargs["with_ios"])
        with patch.object(build, "watchos_simulator_deploy", return_value=("P", "W", PHONE_ID)) as sim:
            self._main("watchos_sim_deploy", "existing")
        self.assertTrue(sim.call_args.kwargs["use_existing"])
        self.assertFalse(sim.call_args.kwargs["with_ios"])
        with patch.object(build, "watchos_simulator_deploy", return_value=("P", "W", PHONE_ID)) as sim:
            self._main("watchos_sim_deploy", "with_ios")
        self.assertTrue(sim.call_args.kwargs["with_ios"])

    def test_with_ios_is_only_for_the_watchos_actions(self):
        err = io.StringIO()
        with patch.object(build, "wearos_device_deploy") as wear, contextlib.redirect_stderr(err), \
                self.assertRaises(SystemExit):
            self._main("wearos_deploy", "with_ios")
        wear.assert_not_called()
        self.assertIn("with_ios applies only to watchos_deploy", err.getvalue())

    def test_deploy_errors_exit_cleanly(self):
        err = io.StringIO()
        with patch.object(build, "wearos_device_deploy", side_effect=wd.WatchDeployError("No Wear OS watch found")), \
             contextlib.redirect_stderr(err), self.assertRaises(SystemExit) as ctx:
            self._main("wearos_deploy")
        self.assertEqual(ctx.exception.code, 1)
        self.assertIn("Error: No Wear OS watch found", err.getvalue())

    def test_help_lists_the_deploy_actions(self):
        out = io.StringIO()
        with patch.object(sys, "argv", ["build.py", "--help"]), contextlib.redirect_stdout(out), self.assertRaises(SystemExit):
            build.main()
        for text in ["watchos_deploy", "watchos_sim_deploy", "wearos_deploy", "wearos_sim_deploy",
                     "IOS_WATCH_DEVELOPMENT_MOBILEPROVISION", "serial=<adb serial>", "with_ios",
                     "ONLY the Apple Watch app"]:
            self.assertIn(text, out.getvalue())


if __name__ == "__main__":
    unittest.main()

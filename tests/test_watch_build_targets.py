"""Command construction and dispatch for the watch companion targets.

These run anywhere: no Xcode, Gradle or SDK is started (subprocess is mocked).
"""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import plistlib
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("frontend_targets", ROOT / "scripts/build_targets.py")
targets = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = targets
spec.loader.exec_module(targets)

WATCH_TARGETS = ["watchos", "watchos_sim", "watchos_sim_intel", "wearos", "wearos_sim"]


def write_plist(path, data):
    path.mkdir(parents=True, exist_ok=True)
    with (path / "Info.plist").open("wb") as f:
        plistlib.dump(data, f)


def fake_ios_app(root, bundle_id="com.UBSEDS.GS26"):
    app = root / "dist" / "GS26.app"
    write_plist(app, {"CFBundleIdentifier": bundle_id})
    return app


def fake_watch_product(root, name, debug, companion="com.UBSEDS.GS26"):
    product = targets.watchos_product(root, name, debug)
    write_plist(product, {"CFBundleIdentifier": "com.UBSEDS.GS26.watchkitapp",
                          "WKCompanionAppBundleIdentifier": companion, "WKApplication": True})
    (product / "GS26Watch").write_bytes(b"binary")
    return product


def quiet():
    return contextlib.redirect_stdout(io.StringIO())


class Inventory(unittest.TestCase):
    def test_watch_targets_follow_mobile_naming_and_groups(self):
        for name in WATCH_TARGETS:
            self.assertIn(name, targets.TARGETS)
        self.assertEqual({n for n in targets.expand_targets("all") if targets.TARGETS[n].watch}, {"watchos", "wearos"})
        self.assertEqual({n for n in targets.expand_targets("sims") if targets.TARGETS[n].watch},
                         {"watchos_sim", "watchos_sim_intel", "wearos_sim"})

    def test_architectures(self):
        t = targets.TARGETS
        self.assertEqual(t["watchos"].watch_archs, ("arm64_32", "arm64"))
        self.assertEqual(t["watchos_sim"].watch_archs, ("arm64",))
        self.assertEqual(t["watchos_sim_intel"].watch_archs, ("x86_64",))
        self.assertEqual(t["wearos"].watch_archs, ("arm64-v8a", "armeabi-v7a"))
        self.assertEqual(t["wearos_sim"].watch_archs, ("x86_64",))
        # The Apple watch targets build the matching iOS phone app with Rust;
        # the Wear app is Kotlin only.
        self.assertEqual(t["watchos"].triple, t["ios"].triple)
        self.assertEqual(t["watchos_sim"].triple, t["ios_sim"].triple)
        self.assertEqual(t["watchos_sim_intel"].triple, t["ios_sim_intel"].triple)
        self.assertIsNone(t["wearos"].triple)
        self.assertIsNone(t["wearos_sim"].triple)

    def test_watchos_requires_macos_and_wear_requires_sdk_or_docker(self):
        with patch.object(targets.platform, "system", return_value="Linux"):
            for name in ["watchos", "watchos_sim", "watchos_sim_intel"]:
                self.assertIn("macOS", targets.native_requirement(targets.TARGETS[name]))
            with patch.dict(os.environ, {"ANDROID_HOME": "", "ANDROID_SDK_ROOT": ""}):
                self.assertIn("Android SDK", targets.native_requirement(targets.TARGETS["wearos"]))
            with patch.dict(os.environ, {"ANDROID_HOME": "/sdk"}):
                self.assertIsNone(targets.native_requirement(targets.TARGETS["wearos_sim"]))
        self.assertEqual(targets.TARGETS["wearos"].docker, "android")

    def test_list_shows_watch_targets_without_building(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertTrue(targets.handle_target_args(["--target", "list"], Path("/tmp"), Mock(), Mock()))
        text = out.getvalue()
        self.assertIn("watchos_sim_intel", text)
        self.assertIn("(none: Kotlin only)", text)
        self.assertIn("arm64_32 arm64", text)


class WatchOSCommands(unittest.TestCase):
    def test_xcode_commands(self):
        root = Path("/w")
        cases = {
            ("watchos", False): ("watchos", "generic/platform=watchOS", "ARCHS=arm64_32 arm64", "Release"),
            ("watchos_sim", True): ("watchsimulator", "generic/platform=watchOS Simulator", "ARCHS=arm64", "Debug"),
            ("watchos_sim_intel", False): ("watchsimulator", "generic/platform=watchOS Simulator", "ARCHS=x86_64", "Release"),
        }
        for (name, debug), (sdk, destination, archs, config) in cases.items():
            with self.subTest(name=name):
                gen, build = targets.watchos_build_commands(root, name, debug)
                self.assertEqual(gen, ["xcodegen", "generate", "--spec", "/w/watch/apple/project.yml", "--project", "/w/watch/apple"])
                self.assertEqual(build[:3], ["xcodebuild", "-project", "/w/watch/apple/GS26Watch.xcodeproj"])
                for flag, value in [("-scheme", "GS26Watch"), ("-sdk", sdk), ("-destination", destination),
                                    ("-configuration", config), ("-derivedDataPath", f"/w/target/watch/{name}")]:
                    self.assertEqual(build[build.index(flag) + 1], value)
                self.assertIn(archs, build)
                self.assertIn("ONLY_ACTIVE_ARCH=NO", build)
                self.assertIn("CODE_SIGNING_ALLOWED=NO", build)
                self.assertEqual(build[-1], "build")
                self.assertEqual(targets.watchos_product(root, name, debug),
                                 Path(f"/w/target/watch/{name}/Build/Products/{config}-{sdk}/GS26Watch.app"))

    def test_xcodegen_spec_matches_scheme_and_companion(self):
        spec_text = (ROOT / "watch/apple/project.yml").read_text()
        self.assertIn("GS26Watch:", spec_text)
        self.assertIn("platform: watchOS", spec_text)
        with (ROOT / "watch/apple/GS26Watch/Info.plist").open("rb") as f:
            info = plistlib.load(f)
        self.assertEqual(info["WKCompanionAppBundleIdentifier"], "com.UBSEDS.GS26")

    def test_embed_checks_companion_and_replaces_previous_copy(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            app = fake_ios_app(root)
            product = fake_watch_product(root, "watchos", False)
            stale = app / "Watch" / "GS26Watch.app" / "old"
            stale.parent.mkdir(parents=True)
            stale.write_text("x")
            embedded = targets.embed_watch_app(product, app)
            self.assertEqual(embedded, app / "Watch" / "GS26Watch.app")
            self.assertTrue((embedded / "GS26Watch").is_file())
            self.assertFalse(stale.exists())
            other = fake_ios_app(Path(d) / "other", "com.example.Other")
            with self.assertRaisesRegex(RuntimeError, "companion id"):
                targets.embed_watch_app(product, other)

    def test_simulator_build_embeds_and_reseals(self):
        for name in ["watchos_sim", "watchos_sim_intel"]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as d, quiet():
                root = Path(d)
                app = fake_ios_app(root)
                fake_watch_product(root, name, True)
                build_app, package_ios = Mock(), Mock()
                with patch.object(targets.platform, "system", return_value="Darwin"), \
                     patch.object(targets.subprocess, "run") as run:
                    self.assertTrue(targets.handle_target_args(["--target", name, "--debug"], root, build_app, package_ios))
                build_app.assert_called_once_with(root, platform_name="ios", rust_target=targets.TARGETS[name].triple,
                                                  debug_mode=True, max_size=False, android_package_type="apk", sign_macos=True)
                commands = [c.args[0] for c in run.call_args_list]
                self.assertEqual(commands[0], ["rustup", "target", "add", targets.TARGETS[name].triple])
                self.assertEqual(commands[1][0], "xcodegen")
                self.assertEqual(commands[2][0], "xcodebuild")
                self.assertEqual(commands[3], ["codesign", "--force", "--sign", "-", "--timestamp=none", str(app / "Watch/GS26Watch.app")])
                self.assertEqual(commands[4], ["codesign", "--force", "--sign", "-", "--timestamp=none", str(app)])
                self.assertTrue((app / "Watch/GS26Watch.app/GS26Watch").is_file())
                self.assertTrue((root / "artifacts" / name / "dist/GS26.app/Watch/GS26Watch.app").is_dir())
                package_ios.assert_not_called()

    def test_unsigned_device_build_embeds_without_signing(self):
        with tempfile.TemporaryDirectory() as d, quiet():
            root = Path(d)
            app = fake_ios_app(root)
            fake_watch_product(root, "watchos", False)
            package_ios = Mock()
            with patch.object(targets.platform, "system", return_value="Darwin"), \
                 patch.object(targets.subprocess, "run") as run:
                targets.handle_target_args(["--target", "watchos", "--no-sign"], root, Mock(), package_ios)
            self.assertNotIn("codesign", [c.args[0][0] for c in run.call_args_list])
            self.assertTrue((app / "Watch/GS26Watch.app").is_dir())
            package_ios.assert_not_called()

    def test_signed_device_build_hands_the_watch_app_to_the_ios_signer_once(self):
        with tempfile.TemporaryDirectory() as d, quiet():
            root = Path(d)
            fake_ios_app(root)
            product = fake_watch_product(root, "watchos", False)
            package_ios = Mock()
            with patch.object(targets.platform, "system", return_value="Darwin"), \
                 patch.object(targets.subprocess, "run"):
                targets.handle_target_args(["--target", "watchos"], root, Mock(), package_ios)
            package_ios.assert_called_once_with(root, sign_kind="distribution", debug_mode=False, watch_app=product)

    def test_final_is_refused_for_watch_targets(self):
        for name in ["watchos", "wearos"]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as d, quiet(), \
                 patch.object(targets.platform, "system", return_value="Darwin"), \
                 patch.dict(os.environ, {"ANDROID_HOME": "/sdk"}), \
                 patch.object(targets.subprocess, "run") as run, \
                 contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaisesRegex(SystemExit, name):
                    targets.handle_target_args(["--target", name, "--final"], Path(d), Mock(), Mock(), Mock())
                run.assert_not_called()

    def test_linux_refuses_explicit_watchos(self):
        with patch.object(targets.platform, "system", return_value="Linux"), quiet(), contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                targets.handle_target_args(["--target", "watchos_sim", "--dry-run"], Path("/tmp"), Mock(), Mock())


class WearOSCommands(unittest.TestCase):
    def test_debug_command_runs_core_tests_and_debug_apk(self):
        with patch.object(targets.platform, "system", return_value="Linux"):
            command = targets.wearos_build_command(Path("/w"), "wearos_sim", True)
        self.assertEqual(command, ["/w/watch/wear/gradlew", "-p", "/w/watch/wear", "--no-daemon", "--console=plain",
                                   f"-Pgs26.applicationId={targets.wear_application_id(ROOT)}", ":core:test", ":app:assembleDebug"])
        self.assertEqual(targets.wearos_product(Path("/w"), True), Path("/w/watch/wear/app/build/outputs/apk/debug/app-debug.apk"))
        self.assertEqual(targets.wearos_build_command(Path("/w"), "wearos", False)[-1], ":app:assembleRelease")

    def test_application_id_follows_dioxus_bundle_identifier(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "Dioxus.toml").write_text('[bundle]\nidentifier = "org.example.Phone"\n')
            self.assertEqual(targets.wear_application_id(Path(d)), "org.example.Phone")
            self.assertEqual(targets.wear_application_id(Path(d) / "missing"), "com.UBSEDS.GS26")

    def test_release_signing_uses_the_phone_key_through_the_environment(self):
        prefix = "ORG_GRADLE_PROJECT_android.injected.signing."
        with tempfile.TemporaryDirectory() as d:
            password = Path(d) / "pw.txt"
            password.write_text("ci-secret\n")
            ci = {"SEDS_GS_CI_ANDROID_KEYSTORE": "/ci/dev.p12", "SEDS_GS_CI_ANDROID_KEYSTORE_PASSWORD_FILE": str(password)}
            self.assertEqual(targets.wearos_signing_properties(ci), {
                prefix + "store.file": "/ci/dev.p12", prefix + "store.password": "ci-secret",
                prefix + "key.alias": "androiddebugkey", prefix + "key.password": "ci-secret"})
        local = {"ANDROID_KEYSTORE_PATH": "/k/upload.jks", "ANDROID_KEY_ALIAS": "upload", "ANDROID_KEYSTORE_PASSWORD": "pw"}
        self.assertEqual(targets.wearos_signing_properties(local)[prefix + "key.password"], "pw")
        self.assertEqual(targets.wearos_signing_properties({}), {})
        with tempfile.TemporaryDirectory() as d, quiet() as out, patch.dict(os.environ, {"ANDROID_HOME": "/sdk", **local}):
            root = Path(d)
            apk = targets.wearos_product(root, False)
            apk.parent.mkdir(parents=True)
            apk.write_bytes(b"apk")
            with patch.object(targets.subprocess, "run") as run:
                targets.handle_target_args(["--target", "wearos"], root, Mock(), Mock())
            command, kwargs = run.call_args.args[0], run.call_args.kwargs
            self.assertFalse(any("pw" in part for part in command))
            self.assertEqual(kwargs["env"][prefix + "store.password"], "pw")
            self.assertNotIn("pw", out.getvalue().replace("/sdk", ""))
            with patch.object(targets.subprocess, "run") as run:
                targets.handle_target_args(["--target", "wearos", "--no-sign"], root, Mock(), Mock())
            self.assertNotIn("env", run.call_args.kwargs)

    def test_wear_build_copies_apk_without_rust(self):
        for name in ["wearos", "wearos_sim"]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as d, quiet(), \
                 patch.dict(os.environ, {"ANDROID_HOME": "/sdk"}):
                root = Path(d)
                apk = targets.wearos_product(root, True)
                apk.parent.mkdir(parents=True)
                apk.write_bytes(b"apk")
                build_app = Mock()
                with patch.object(targets.subprocess, "run") as run:
                    targets.handle_target_args(["--target", name, "--debug"], root, build_app, Mock())
                build_app.assert_not_called()
                commands = [c.args[0] for c in run.call_args_list]
                self.assertEqual(len(commands), 1)
                self.assertTrue(commands[0][0].endswith("gradlew"))
                self.assertEqual((root / "artifacts" / name / "dist" / targets.WEAR_APK_NAME).read_bytes(), b"apk")

    def test_dry_run_prints_gradle_and_runs_nothing(self):
        out = io.StringIO()
        with patch.dict(os.environ, {"ANDROID_HOME": "/sdk"}), patch.object(targets.subprocess, "run") as run, \
             contextlib.redirect_stdout(out):
            targets.handle_target_args(["--target", "wearos,wearos_sim", "--dry-run"], Path("/tmp/x"), Mock(), Mock())
        run.assert_not_called()
        self.assertEqual(out.getvalue().count(":app:assembleRelease"), 2)

    def test_docker_runs_the_wear_build_in_the_android_image(self):
        args = targets.parse_args(["--docker", "--target", "wearos"])
        commands = targets.docker_commands(Path("/tmp/ui"), "wearos", args)
        self.assertIn("seds-gs-ui-builder:android-amd64", commands[1])
        self.assertEqual(commands[1][commands[1].index("build.py") + 1:][:2], ["--target", "wearos"])


class WatchIcons(unittest.TestCase):
    """The committed icons match scripts/generate_watch_icons.py's contract."""

    def test_watchos_app_icon_catalog(self):
        from PIL import Image
        import json
        apple = ROOT / "watch" / "apple"
        catalog = apple / "GS26Watch" / "Assets.xcassets"
        self.assertEqual(json.loads((catalog / "Contents.json").read_text())["info"]["version"], 1)
        iconset = catalog / "AppIcon.appiconset"
        images = json.loads((iconset / "Contents.json").read_text())["images"]
        self.assertEqual(images, [{"filename": "AppIcon.png", "idiom": "universal",
                                   "platform": "watchos", "size": "1024x1024"}])
        with Image.open(iconset / "AppIcon.png") as icon:
            self.assertEqual(icon.format, "PNG")
            self.assertEqual(icon.size, (1024, 1024))
            self.assertEqual(icon.mode, "RGB")  # watchOS rejects icons with alpha
            self.assertNotIn("transparency", icon.info)
            # Everything the circular mask removes is the flat background.
            corners = {icon.getpixel(p) for p in [(0, 0), (1023, 0), (0, 1023), (1023, 1023), (150, 150)]}
            self.assertEqual(corners, {(0x1E, 0x1F, 0x22)})
        spec_text = (apple / "project.yml").read_text()
        self.assertIn("ASSETCATALOG_COMPILER_APPICON_NAME: AppIcon", spec_text)
        self.assertIn('deploymentTarget: "10.0"', spec_text)  # single-size icon needs watchOS 9+

    def test_wear_adaptive_launcher_icons(self):
        from PIL import Image
        res = ROOT / "watch" / "wear" / "app" / "src" / "main" / "res"
        manifest = (res.parent / "AndroidManifest.xml").read_text()
        self.assertIn('android:icon="@mipmap/ic_launcher"', manifest)
        self.assertIn('android:roundIcon="@mipmap/ic_launcher_round"', manifest)
        for name in ["ic_launcher", "ic_launcher_round"]:
            text = (res / "mipmap-anydpi" / f"{name}.xml").read_text()
            self.assertIn("@mipmap/ic_launcher_foreground", text)
            self.assertIn("@color/ic_launcher_background", text)
        self.assertIn("#1E1F22", (res / "values" / "colors.xml").read_text())
        for density, factor in {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}.items():
            with Image.open(res / f"mipmap-{density}" / "ic_launcher_foreground.png") as fg:
                self.assertEqual(fg.size, (round(108 * factor),) * 2, density)


if __name__ == "__main__":
    unittest.main()

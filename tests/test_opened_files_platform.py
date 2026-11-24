"""Replay files opened from other apps (docs/replay-bundle.md, "Opening files
from other apps"): the platform registrations and the code that delivers the
file to src/opened_file.rs. No SDK, device or Apple tool is used; the iOS
script runs through the test PlistBuddy adapter.
"""
import importlib.util
from pathlib import Path
import plistlib
import re
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
ANDROID = "{http://schemas.android.com/apk/res/android}"
MEDIA_TYPE = "application/vnd.seds.gsreplay+zip"
UTI = "com.ubseds.gsreplay"


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


build = load("opened_files_build", "build.py")
ios_tests = load("opened_files_ios_tests", "tests/test_ios_bundle_platform.py")


def assert_apple_declarations(test, info):
    exported = {t["UTTypeIdentifier"]: t for t in info["UTExportedTypeDeclarations"]}
    gsreplay = exported[UTI]
    test.assertEqual(gsreplay["UTTypeConformsTo"], ["public.zip-archive", "public.data"])
    tags = gsreplay["UTTypeTagSpecification"]
    test.assertEqual(tags["public.filename-extension"], ["gsreplay"])
    test.assertEqual(tags["public.mime-type"], [MEDIA_TYPE])
    documents = [d for d in info["CFBundleDocumentTypes"] if UTI in d["LSItemContentTypes"]]
    test.assertEqual(len(documents), 1)
    test.assertEqual(documents[0]["CFBundleTypeRole"], "Viewer")
    test.assertEqual(documents[0]["LSHandlerRank"], "Owner")
    test.assertTrue(documents[0]["CFBundleTypeName"].strip())


class AppleContract(unittest.TestCase):
    def test_macos_plist_declares_and_views_gsreplay(self):
        with (ROOT / "platform/macos/Info.plist").open("rb") as f:
            assert_apple_declarations(self, plistlib.load(f))

    def test_ios_patch_declares_gsreplay_once_and_opens_in_place(self):
        harness = ios_tests.IOSPlistMetadataTests("test_legacy_manual_script_defaults_to_device_labels")
        harness.setUp()
        try:
            app = harness.app(2)
            harness.script((ROOT / "scripts/patch_plist.sh").read_text())
            for _ in range(2):
                build.patch_plist(harness.root, app, ios_target="aarch64-apple-ios")
            info = plistlib.loads((app / "Info.plist").read_bytes())
            assert_apple_declarations(self, info)
            self.assertEqual(len(info["UTExportedTypeDeclarations"]), 1, "idempotent")
            self.assertEqual(len(info["CFBundleDocumentTypes"]), 1, "idempotent")
            self.assertIs(info["LSSupportsOpeningDocumentsInPlace"], True)
            self.assertNotIn("UISupportsDocumentBrowser", info, "the app is not a document browser")
        finally:
            harness.doCleanups()

    def test_tao_starts_security_scope_for_opened_file_urls(self):
        view = (ROOT / "vendor-tao/src/platform_impl/ios/view.rs").read_text()
        handler = view[view.index("fn handle_deep_link"):]
        handler = handler[: handler.index("Event::Opened")]
        self.assertIn("isFileURL", handler)
        self.assertIn("startAccessingSecurityScopedResource", handler)
        self.assertIn("handle_deep_link", (ROOT / "vendor-tao/GROUNDSTATION-PATCH.md").read_text())

    def test_native_main_forwards_opened_events(self):
        main = (ROOT / "src/main.rs").read_text()
        self.assertIn("with_custom_event_handler", main)
        self.assertIn("Event::Opened { urls }", main)
        self.assertIn("opened_file::offer_urls(urls)", main)
        self.assertIn("opened_file::offer_launch_arguments()", main)


class AndroidContract(unittest.TestCase):
    def setUp(self):
        root = ET.parse(ROOT / "platform/android/AndroidManifest.xml").getroot()
        activities = {a.get(ANDROID + "name"): a for a in root.find("application").findall("activity")}
        self.activity = activities["dev.dioxus.main.MainActivity"]

    def filters(self, action):
        found = []
        for f in self.activity.findall("intent-filter"):
            if action in {a.get(ANDROID + "name") for a in f.findall("action")}:
                found.append(f)
        return found

    @staticmethod
    def data(f, attribute):
        return {d.get(ANDROID + attribute) for d in f.findall("data") if d.get(ANDROID + attribute)}

    def test_single_activity_receives_later_files(self):
        self.assertEqual(self.activity.get(ANDROID + "launchMode"), "singleTask")

    def test_view_filters_cover_media_type_and_extension(self):
        views = self.filters("android.intent.action.VIEW")
        for f in views:
            categories = {c.get(ANDROID + "name") for c in f.findall("category")}
            self.assertIn("android.intent.category.DEFAULT", categories)
            self.assertIn("android.intent.category.BROWSABLE", categories)
            self.assertEqual(self.data(f, "scheme"), {"content", "file"})
        typed = [f for f in views if MEDIA_TYPE in self.data(f, "mimeType")]
        self.assertEqual(len(typed), 1)
        self.assertEqual(self.data(typed[0], "pathPattern"), set(), "the media type alone decides")
        by_name = [f for f in views if ".*\\\\.gsreplay" in self.data(f, "pathPattern")]
        self.assertEqual(len(by_name), 1)
        self.assertTrue({"application/zip", "application/octet-stream"} <= self.data(by_name[0], "mimeType"))
        self.assertEqual(self.data(by_name[0], "host"), {"*"}, "pathPattern applies only with a host")
        self.assertIn(".*\\\\.seds-replay\\\\.json", self.data(by_name[0], "pathPattern"))

    def test_raw_manifest_escapes_the_dot_for_android(self):
        raw = (ROOT / "platform/android/AndroidManifest.xml").read_text()
        self.assertIn('android:pathPattern=".*\\\\.gsreplay"', raw)

    def test_send_filter_takes_shared_bundles(self):
        sends = self.filters("android.intent.action.SEND")
        self.assertEqual(len(sends), 1)
        self.assertIn(MEDIA_TYPE, self.data(sends[0], "mimeType"))
        self.assertIn("android.intent.category.DEFAULT", {c.get(ANDROID + "name") for c in sends[0].findall("category")})

    def test_launcher_filter_is_unchanged(self):
        mains = self.filters("android.intent.action.MAIN")
        self.assertEqual(len(mains), 1)
        self.assertEqual({c.get(ANDROID + "name") for c in mains[0].findall("category")}, {"android.intent.category.LAUNCHER"})

    def test_generated_main_activity_forwards_intents_once(self):
        with tempfile.TemporaryDirectory() as temporary:
            kotlin = Path(temporary) / "app/src/main/kotlin/dev/dioxus/main"
            kotlin.mkdir(parents=True)
            activity = kotlin / "MainActivity.kt"
            generated = "package dev.dioxus.main\n\ntypealias BuildConfig = com.UBSEDS.GS26.BuildConfig\n\nclass MainActivity : WryActivity()\n"
            activity.write_text(generated)
            for _ in range(2):
                build._patch_generated_android_sources(Path(temporary))
            raw = activity.read_text()
        self.assertTrue(raw.startswith("package dev.dioxus.main\n\ntypealias BuildConfig = com.UBSEDS.GS26.BuildConfig\n"))
        self.assertEqual(raw.count("OpenedFileBridge.accept(this, intent)"), 2)
        self.assertIn("override fun onNewIntent(intent: android.content.Intent)", raw)
        self.assertIn("setIntent(intent)", raw)
        self.assertIn("if (savedInstanceState == null)", raw, "recreation does not reopen the file")

    def test_unknown_main_activity_shape_fails_loudly(self):
        with tempfile.TemporaryDirectory() as temporary:
            kotlin = Path(temporary) / "app/src/main/kotlin/dev/dioxus/main"
            kotlin.mkdir(parents=True)
            (kotlin / "MainActivity.kt").write_text("class MainActivity : SomethingElse()\n")
            with self.assertRaisesRegex(RuntimeError, "MainActivity.kt"):
                build._patch_generated_android_sources(Path(temporary))

    def test_bridge_and_rust_share_the_native_symbol_and_ceiling(self):
        bridge = (ROOT / "platform/android/kotlin/com/ubseds/gs26/OpenedFileBridge.kt").read_text()
        rust = (ROOT / "src/opened_file.rs").read_text()
        self.assertIn("external fun nativeOnOpenedFile()", bridge)
        self.assertIn("Java_com_ubseds_gs26_OpenedFileBridge_nativeOnOpenedFile", rust)
        self.assertIn('jni_str!("takePending"), jni_sig!("()Ljava/lang/String;")', rust)
        self.assertIn("fun takePending(): String?", bridge)
        self.assertIn("192L shl 20", bridge)
        self.assertIn("MAX_BYTES: usize = 192 << 20", rust)
        for source in ["Intent.ACTION_VIEW", "Intent.ACTION_SEND", "Intent.EXTRA_STREAM", "openInputStream"]:
            self.assertIn(source, bridge)
        rules = (ROOT / "platform/android/proguard-rules.pro").read_text()
        self.assertIn("-keep class com.ubseds.gs26.OpenedFileBridge { *; }", rules)


class DesktopContract(unittest.TestCase):
    def test_linux_payload_registers_the_type_and_passes_the_file(self):
        import contextlib
        import io
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable = root / "input/groundstation_frontend"
            executable.parent.mkdir()
            executable.write_bytes(b"#!/bin/sh\nexit 0\n")
            with patch.object(build, "_find_linux_app_binary", return_value=executable), \
                 patch.object(build, "dist_dir", return_value=root / "empty-dist"), \
                 contextlib.redirect_stdout(io.StringIO()):
                lease, package = build._stage_linux_app_payload(ROOT, "x86_64-unknown-linux-gnu", False)
            try:
                desktop = (package / "usr/share/applications/ubseds-groundstation.desktop").read_text()
                mime = ET.parse(package / "usr/share/mime/packages/ubseds-groundstation.xml").getroot()
            finally:
                lease.cleanup()
        self.assertIn("Exec=ubseds-groundstation %f\n", desktop)
        self.assertIn(f"MimeType={MEDIA_TYPE};\n", desktop)
        ns = "{http://www.freedesktop.org/standards/shared-mime-info}"
        entry = mime.find(ns + "mime-type")
        self.assertEqual(entry.get("type"), MEDIA_TYPE)
        self.assertEqual(entry.find(ns + "glob").get("pattern"), "*.gsreplay")
        self.assertEqual(entry.find(ns + "sub-class-of").get("type"), "application/zip")

    def test_windows_installer_associates_and_uninstall_removes(self):
        with tempfile.TemporaryDirectory() as directory:
            script = Path(directory) / "installer.nsi"
            build._write_windows_nsis_script(script, Path("C:/stage"), Path("C:/icon.ico"), Path("C:/out.exe"))
            raw = script.read_text()
        install, uninstall = raw.split('Section "Uninstall"')
        progid = build.WINDOWS_GSREPLAY_PROGID
        self.assertIn(f'WriteRegStr HKCU "Software\\Classes\\.gsreplay" "" "{progid}"', install)
        self.assertIn(f'"Content Type" "{MEDIA_TYPE}"', install)
        command = re.search(r'Classes\\' + re.escape(progid) + r'\\shell\\open\\command" "" (.+)', install).group(1)
        self.assertEqual(command, f"'\"$INSTDIR\\{build.WINDOWS_APP_NAME}.exe\" \"%1\"'")
        self.assertIn(f'DeleteRegKey HKCU "Software\\Classes\\{progid}"', uninstall)
        self.assertIn('DeleteRegKey /ifempty HKCU "Software\\Classes\\.gsreplay"', uninstall)


class DocumentationContract(unittest.TestCase):
    def test_replay_bundle_doc_explains_testing_per_platform(self):
        doc = (ROOT / "docs/replay-bundle.md").read_text()
        section = doc[doc.index("## Opening files from other apps"):]
        for needle in ["adb shell am start", MEDIA_TYPE, UTI, "groundstation_frontend", "192 MiB", "AirDrop"]:
            self.assertIn(needle, section)


if __name__ == "__main__":
    unittest.main()

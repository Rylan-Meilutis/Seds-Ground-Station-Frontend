"""Native builds can open and use the voice, camera and recording pages.

Checks the permission strings, transport exceptions, entitlements, Android
WebView settings and the in-page explanations. No SDK or device is used.
"""
import importlib.util
import os
from pathlib import Path
import plistlib
import re
import sys
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


build = load("native_media_pages_build", "build.py")
ANDROID = "{http://schemas.android.com/apk/res/android}name"


class ApplePermissionContracts(unittest.TestCase):
    def setUp(self):
        with (ROOT / "platform/macos/Info.plist").open("rb") as f:
            self.info = plistlib.load(f)

    def test_bundle_plist_used_by_ios_and_macos_carries_capture_strings(self):
        dioxus = (ROOT / "Dioxus.toml").read_text()
        self.assertEqual(dioxus.count('info_plist_path = "platform/macos/Info.plist"'), 2)
        for key in ["NSCameraUsageDescription", "NSMicrophoneUsageDescription"]:
            self.assertTrue(self.info.get(key, "").strip(), key)

    def test_web_content_may_load_lan_station_documents(self):
        ats = self.info.get("NSAppTransportSecurity", {})
        self.assertIs(ats.get("NSAllowsLocalNetworking"), True)
        self.assertIs(ats.get("NSAllowsArbitraryLoadsInWebContent"), True)
        self.assertNotIn("NSAllowsArbitraryLoads", ats, "only web content is exempt")

    def test_ios_patch_script_sets_capture_strings_and_ats(self):
        script = (ROOT / "scripts/patch_plist.sh").read_text()
        for key in ["NSCameraUsageDescription", "NSMicrophoneUsageDescription"]:
            self.assertRegex(script, rf'set_string "{key}" \\\n\s+"[^"]+"')
        self.assertIn('ensure_dict "NSAppTransportSecurity"', script)
        self.assertIn('set_bool "NSAppTransportSecurity:NSAllowsLocalNetworking" "true"', script)
        self.assertIn('set_bool "NSAppTransportSecurity:NSAllowsArbitraryLoadsInWebContent" "true"', script)

    def test_macos_hardened_runtime_signature_carries_device_entitlements(self):
        path = ROOT / "platform/macos/entitlements.plist"
        with path.open("rb") as f:
            entitlements = plistlib.load(f)
        self.assertIs(entitlements.get("com.apple.security.device.camera"), True)
        self.assertIs(entitlements.get("com.apple.security.device.audio-input"), True)
        self.assertNotIn("com.apple.security.app-sandbox", entitlements)
        with patch.dict(os.environ, {"MACOS_ENTITLEMENTS": ""}):
            self.assertEqual(build._macos_entitlements_path(ROOT), path)
        with tempfile.TemporaryDirectory() as temporary:
            custom = Path(temporary) / "custom.plist"
            custom.write_bytes(path.read_bytes())
            with patch.dict(os.environ, {"MACOS_ENTITLEMENTS": str(custom)}):
                self.assertEqual(build._macos_entitlements_path(ROOT), custom)

    def test_webview_shim_is_built_and_linked_for_apple_targets(self):
        build_rs = (ROOT / "build.rs").read_text()
        self.assertIn('"assets/MediaWebViewShim.m"', build_rs)
        self.assertIn('out_dir.join("MediaWebViewShim.o")', build_rs)
        for framework in ["Security", "WebKit"]:
            self.assertIn(f'cargo:rustc-link-lib=framework={framework}', build_rs)
        shim = (ROOT / "assets/MediaWebViewShim.m").read_text()
        for symbol in ["gs26_webview_install_media_policy", "gs26_webview_trust_certificate"]:
            self.assertIn(f"void {symbol}(", shim)
            self.assertIn(f"fn {symbol}(", (ROOT / "src/native_media.rs").read_text())
        # Only an exact pinned certificate is accepted; everything else is default.
        self.assertIn("[presented isEqualToData:pinned]", shim)
        self.assertIn("NSURLSessionAuthChallengePerformDefaultHandling", shim)


class WebViewDefaults(unittest.TestCase):
    """The app relies on wry defaults for playback and capture prompts."""

    def wry(self):
        found = sorted((Path.home() / ".cargo/registry/src").glob("*/wry-0.53.*/src"))
        if not found:
            self.skipTest("wry sources are not in the local cargo registry")
        return found[-1]

    def test_app_never_turns_off_gesture_free_autoplay(self):
        for path in [ROOT / "vendor-dioxus-desktop/src/webview.rs", ROOT / "src/main.rs"]:
            self.assertNotIn("with_autoplay(false)", path.read_text(), path.name)

    def test_wkwebview_plays_inline_and_grants_capture_requests(self):
        wry = self.wry()
        webview = (wry / "wkwebview/mod.rs").read_text()
        self.assertIn('ns_string!("allowsInlineMediaPlayback")', webview)
        self.assertIn("setMediaTypesRequiringUserActionForPlayback(WKAudiovisualMediaTypes::None)", webview)
        delegate = (wry / "wkwebview/class/wry_web_view_ui_delegate.rs").read_text()
        start = delegate.index("requestMediaCapturePermissionForOrigin")
        self.assertIn("WKPermissionDecision::Grant", delegate[start:start + 800])
        navigation = (wry / "wkwebview/class/wry_navigation_delegate.rs").read_text()
        self.assertNotIn("didReceiveAuthenticationChallenge", navigation,
                         "the shim adds this method; revisit it if wry starts implementing it")

    def test_webview2_args_keep_wry_defaults(self):
        wry = self.wry()
        defaults = re.search(r'let default_args = "([^"]+)"', (wry / "webview2/mod.rs").read_text()).group(1)
        self.assertIn(f'"{defaults} --autoplay-policy=no-user-gesture-required"',
                      (ROOT / "src/native_media.rs").read_text().replace("\n", ""))


class AndroidPermissionContracts(unittest.TestCase):
    def test_manifest_requests_capture_permissions_without_requiring_hardware(self):
        root = ET.parse(ROOT / "platform/android/AndroidManifest.xml").getroot()
        permissions = {p.get(ANDROID) for p in root.findall("uses-permission")}
        for name in ["CAMERA", "RECORD_AUDIO", "MODIFY_AUDIO_SETTINGS", "INTERNET"]:
            self.assertIn(f"android.permission.{name}", permissions)
        features = {f.get(ANDROID): f.get("{http://schemas.android.com/apk/res/android}required") for f in root.findall("uses-feature")}
        self.assertEqual(features.get("android.hardware.camera"), "false")
        self.assertEqual(features.get("android.hardware.microphone"), "false")

    def test_cleartext_station_documents_are_permitted(self):
        root = ET.parse(ROOT / "platform/android/res/xml/network_security_config.xml").getroot()
        base = root.find("base-config")
        self.assertEqual(base.get("cleartextTrafficPermitted"), "true")
        anchors = {c.get("src") for c in base.findall("trust-anchors/certificates")}
        self.assertEqual(anchors, {"system", "user"}, "an installed station certificate is trusted")

    def test_generated_webview_allows_station_frames_inside_the_https_document(self):
        template = (
            "    init {\n"
            "        settings.javaScriptEnabled = true\n"
            "        settings.domStorageEnabled = true\n"
            "        settings.setGeolocationEnabled(true)\n"
            "        settings.databaseEnabled = true\n"
            "        settings.mediaPlaybackRequiresUserGesture = false\n"
        )
        with tempfile.TemporaryDirectory() as temporary:
            kotlin = Path(temporary) / "app/src/main/kotlin/dev/dioxus/main"
            kotlin.mkdir(parents=True)
            (kotlin / "RustWebView.kt").write_text(template)
            build._patch_generated_android_sources(Path(temporary))
            build._patch_generated_android_sources(Path(temporary))
            patched = (kotlin / "RustWebView.kt").read_text()
        self.assertEqual(patched.count("settings.mixedContentMode = WebSettings.MIXED_CONTENT_ALWAYS_ALLOW"), 1)
        self.assertNotIn("databaseEnabled", patched)
        self.assertIn("mediaPlaybackRequiresUserGesture = false", patched, "autoplay stays gesture-free")
        with self.assertRaises(RuntimeError):
            build._allow_station_media_frames("class RustWebView {}")

    def test_wry_template_still_matches_the_patch_marker(self):
        registry = Path.home() / ".cargo/registry/src"
        templates = sorted(registry.glob("*/wry-0.53.*/src/android/kotlin/RustWebView.kt"))
        if not templates:
            self.skipTest("wry sources are not in the local cargo registry")
        raw = templates[-1].read_text()
        self.assertIn("settings.domStorageEnabled = true\n", raw)
        self.assertIn("settings.mediaPlaybackRequiresUserGesture = false", raw)
        client = templates[-1].with_name("RustWebChromeClient.kt").read_text()
        self.assertIn("override fun onPermissionRequest", client)
        self.assertIn("RECORD_AUDIO", client)

    def test_tls_errors_are_accepted_only_for_the_unverified_saved_station(self):
        client = (ROOT / "platform/android/kotlin/dev/dioxus/main/RustWebViewClient.kt").read_text()
        start = client.index("override fun onReceivedSslError(")
        body = client[start:client.index("private fun sameOrigin", start)]
        self.assertIn("loadSkipTlsVerify(station)", body)
        self.assertIn("sameOrigin(Uri.parse(error.url), Uri.parse(station))", body)
        self.assertLess(body.index("loadSkipTlsVerify"), body.index("handler.proceed()"))
        self.assertIn("handler.cancel()", body)


class StationPageExplanations(unittest.TestCase):
    def test_voice_explains_missing_capture_before_join(self):
        voice = (ROOT / "station_ui/scripts/voice.js").read_text()
        self.assertIn("function voiceUnavailableReason()", voice)
        join = voice[voice.index("$('join').onclick"):]
        self.assertLess(join.index("voiceUnavailableReason()"), join.index("getUserMedia"))
        for capability in ["isSecureContext", "getUserMedia", "audioWorklet"]:
            self.assertIn(capability, voice[voice.index("function voiceUnavailableReason"):voice.index("$('join').onclick")])

    def test_cameras_explain_missing_webrtc_and_capture(self):
        media = (ROOT / "station_ui/pages/media.html").read_text()
        self.assertIn("const webrtcSupported = typeof RTCPeerConnection === 'function';", media)
        refresh = media[media.index("async function refreshVideo()"):]
        self.assertLess(refresh.index("if (!webrtcSupported)"), refresh.index("addCamera("))
        viewfinder = media[media.index("async function openViewfinder()"):]
        self.assertLess(viewfinder.index("captureUnavailableReason()"), viewfinder.index("getUserMedia"))
        player = (ROOT / "station_ui/pages/player.html").read_text()
        self.assertLess(player.index("typeof RTCPeerConnection!=='function'"), player.index("new RTCPeerConnection"))

    def test_program_falls_back_between_mse_and_native_hls(self):
        program = (ROOT / "station_ui/pages/program.html").read_text()
        self.assertIn("Hls.isSupported()", program)
        self.assertIn("canPlayType('application/vnd.apple.mpegurl')", program)
        manifest = (ROOT / "station_ui/manifest.json").read_text()
        self.assertIn("hls.min.js", manifest)


if __name__ == "__main__":
    unittest.main()

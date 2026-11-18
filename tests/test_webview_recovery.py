"""The app recovers by itself when the platform kills or blanks its webview.

Every native webview can lose its web content process, usually while the app
is in the background; the window then shows only its background colour. These
checks cover the platform hooks, the Android overlay as build.py installs it
into a generated project, and the reload path in dioxus-desktop. No SDK or
device is used.
"""
import importlib.util
from pathlib import Path
import re
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
LOG_LINE = "[webview] content process terminated, reloading"


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


build = load("webview_recovery_build", "build.py")


def wry_sources():
    found = sorted((Path.home() / ".cargo/registry/src").glob("*/wry-0.53.*/src"))
    return found[-1] if found else None


def body_of(source, signature, end_marker):
    start = source.index(signature)
    return source[start:source.index(end_marker, start + len(signature))]


class AppleWebKit(unittest.TestCase):
    def setUp(self):
        self.shim = (ROOT / "assets/MediaWebViewShim.m").read_text()

    def test_content_process_termination_reaches_the_reload_path(self):
        wry = wry_sources()
        if wry is None:
            self.skipTest("wry sources are not in the local cargo registry")
        # wry's navigation delegate already declares the method, so the shim must
        # not add its own (class_addMethod would fail); dioxus-desktop hooks it.
        delegate = (wry / "wkwebview/class/wry_navigation_delegate.rs").read_text()
        self.assertIn("webViewWebContentProcessDidTerminate:", delegate)
        self.assertNotIn("@selector(webViewWebContentProcessDidTerminate:)", self.shim)
        webview = (ROOT / "vendor-dioxus-desktop/src/webview.rs").read_text()
        hook = body_of(webview, "with_on_web_content_process_terminate_handler", "});")
        self.assertIn("UserWindowEvent::RecoverWebview", hook)
        launch = (ROOT / "vendor-dioxus-desktop/src/launch.rs").read_text()
        self.assertIn("UserWindowEvent::RecoverWebview { id, reason } => app.recover_webviews(id, &reason)", launch)

    def test_shim_checks_the_page_each_time_the_app_becomes_active(self):
        self.assertIn("void gs26_webview_install_recovery(void *webview_ptr)", self.shim)
        self.assertIn("UIApplicationDidBecomeActiveNotification", self.shim)
        self.assertIn("NSApplicationDidBecomeActiveNotification", self.shim)
        probe = body_of(self.shim, "static void GS26ProbePage(", "\n}\n")
        self.assertIn("webView.URL == nil", probe, "a page with no document is dead")
        self.assertIn("error != nil", probe, "a page that cannot answer is dead")
        self.assertIn("getElementById('main')", self.shim, "dioxus mounts into #main")
        check = body_of(self.shim, "static void GS26CheckPageAfterActivation(", "\nvoid gs26_webview_install_recovery")
        self.assertIn("GS26RecoveryGraceSeconds", check, "an empty page gets time to render first")
        reload = body_of(self.shim, "static void GS26RequestRecoveryReload(", "\n}\n")
        self.assertIn("GS26RecoveryMinIntervalSeconds", reload, "no reload loop")
        self.assertIn(LOG_LINE, reload)
        # The reload goes through Rust: the navigation policy only lets an armed
        # reload of the app document through.
        self.assertIn("gs26_webview_request_reload(reason.UTF8String)", reload)
        self.assertNotIn("[webView reload]", self.shim)

    def test_rust_declares_and_exports_the_shim_symbols(self):
        rust = (ROOT / "src/webview_recovery.rs").read_text()
        self.assertIn("fn gs26_webview_install_recovery(webview: *mut c_void);", rust)
        self.assertRegex(rust, r"#\[unsafe\(no_mangle\)\]\s+pub extern \"C\" fn gs26_webview_request_reload\(")
        self.assertIn("void gs26_webview_request_reload(const char *reason);", self.shim)

    def test_build_links_the_frameworks_the_activation_observer_needs(self):
        build_rs = (ROOT / "build.rs").read_text()
        self.assertIn('"assets/MediaWebViewShim.m"', build_rs)
        self.assertIn("cargo:rustc-link-lib=framework=UIKit", build_rs)
        self.assertIn("cargo:rustc-link-lib=framework=AppKit", build_rs)


class AndroidWebView(unittest.TestCase):
    CLIENT = ROOT / "platform/android/kotlin/dev/dioxus/main/RustWebViewClient.kt"

    def generated_project(self, temporary):
        """A dx-generated project with wry's own client, patched as build.py does."""
        kotlin = Path(temporary) / "app/src/main/kotlin/dev/dioxus/main"
        kotlin.mkdir(parents=True)
        (kotlin / "RustWebViewClient.kt").write_text(
            "class RustWebViewClient(context: Context): WebViewClient() {}\n"
        )
        (kotlin / "RustWebView.kt").write_text(
            "    init {\n"
            "        settings.javaScriptEnabled = true\n"
            "        settings.domStorageEnabled = true\n"
            "        settings.databaseEnabled = true\n"
            "    }\n"
        )
        build._merge_tree(ROOT / "platform/android/kotlin", Path(temporary) / "app/src/main/kotlin")
        build._patch_generated_android_sources(Path(temporary))
        return kotlin

    def test_build_installs_the_recovering_client_over_the_generated_one(self):
        with tempfile.TemporaryDirectory() as temporary:
            kotlin = self.generated_project(temporary)
            client = (kotlin / "RustWebViewClient.kt").read_text()
            webview = (kotlin / "RustWebView.kt").read_text()
        self.assertEqual(client, self.CLIENT.read_text())
        self.assertIn("override fun onRenderProcessGone(", client)
        self.assertIn("settings.mixedContentMode", webview, "the existing RustWebView patch still applies")

    def test_renderer_death_is_handled_and_the_app_restarts_in_the_foreground(self):
        client = self.CLIENT.read_text()
        gone = body_of(client, "override fun onRenderProcessGone(", "\n    }\n")
        self.assertIn(LOG_LINE, gone)
        self.assertIn("view.destroy()", gone, "a dead WebView is never used again")
        self.assertIn("return true", gone, "handled: the app process survives the renderer")
        self.assertIn("hostIsResumed()", gone)
        self.assertIn("restartWhenResumed = true", gone, "a background death waits for the user")
        restart = body_of(client, "private fun restartApp()", "\n    }\n")
        self.assertIn("FLAG_ACTIVITY_CLEAR_TASK", restart)
        self.assertLess(restart.index("startActivity"), restart.index("Runtime.getRuntime().exit(0)"))

    def test_resume_restarts_timers_and_reloads_a_blank_page(self):
        client = self.CLIENT.read_text()
        resumed = body_of(client, "private fun onHostResumed()", "\n    }\n")
        self.assertIn("view.onResume()", resumed)
        self.assertIn("view.resumeTimers()", resumed)
        self.assertIn("checkPageAfterResume()", resumed)
        self.assertIn("Lifecycle.Event.ON_RESUME", client)
        self.assertIn("getElementById('main')", client)
        check = body_of(client, "private fun checkPageAfterResume()", "\n    /**")
        self.assertIn("RECOVERY_GRACE_MS", check)
        reload = body_of(client, "private fun reloadBlankPage(", "\n    }\n")
        self.assertIn("RECOVERY_MIN_INTERVAL_MS", reload, "no reload loop")
        self.assertIn(LOG_LINE, reload)
        self.assertIn("view.reload()", reload)

    def test_overlay_targets_the_generated_package(self):
        client = self.CLIENT.read_text()
        self.assertTrue(client.startswith("package dev.dioxus.main\n"))
        self.assertIn("watchForResume(view)", body_of(client, "override fun onPageFinished(", "\n    }\n"))


class DesktopWebViews(unittest.TestCase):
    def setUp(self):
        self.rust = (ROOT / "src/webview_recovery.rs").read_text()

    def test_webkitgtk_reloads_after_the_web_process_ends(self):
        linux = body_of(self.rust, '#[cfg(target_os = "linux")]\nmod platform', "\n}\n")
        self.assertIn("connect_web_process_terminated", linux)
        self.assertIn("request_webview_reload", linux)

    def test_webview2_reloads_after_the_renderer_fails(self):
        windows = body_of(self.rust, '#[cfg(target_os = "windows")]\nmod platform', "\n}\n")
        self.assertIn("ProcessFailedEventHandler::create", windows)
        self.assertIn("add_ProcessFailed", windows)
        self.assertIn("request_webview_reload", windows)

    def test_webview2_com_matches_the_version_wry_links(self):
        wry = wry_sources()
        if wry is None:
            self.skipTest("wry sources are not in the local cargo registry")
        wry_manifest = (wry.parent / "Cargo.toml").read_text()
        wry_version = re.search(r'\[target\.[^\]]*windows[^\]]*\.dependencies\.webview2-com\]\nversion = "([^"]+)"',
                                wry_manifest).group(1)
        ours = re.search(r'^webview2-com = "([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M).group(1)
        self.assertEqual(ours.split(".")[:2], wry_version.split(".")[:2])

    def test_recoveries_are_logged_to_the_app_log(self):
        main = (ROOT / "src/main.rs").read_text()
        self.assertIn("with_webview_recovery_logger(debug_log::append)", main)
        recovery = (ROOT / "vendor-dioxus-desktop/src/recovery.rs").read_text()
        self.assertIn(f'format!("{LOG_LINE} ({{reason}})")', recovery)


class ReloadPath(unittest.TestCase):
    """dioxus-desktop serves the reload and remounts the app on the same route."""

    def setUp(self):
        self.vendor = ROOT / "vendor-dioxus-desktop/src"

    def test_navigation_policy_lets_only_an_armed_reload_through(self):
        webview = (self.vendor / "webview.rs").read_text()
        self.assertIn("crate::recovery::allow_app_document_navigation(", webview)
        reload = body_of(webview, "pub(crate) fn reload_document(", "\n    }\n")
        self.assertLess(reload.index("app_reload_armed"), reload.index("webview.reload()"))

    def test_each_served_document_restarts_the_edit_stream(self):
        protocol = (self.vendor / "protocol.rs").read_text()
        index = body_of(protocol, "fn index_request(", "\n}\n")
        self.assertLess(index.index("begin_document()"), index.index("module_loader("))
        webview = (self.vendor / "webview.rs").read_text()
        poll = body_of(webview, "pub fn poll_vdom(&mut self)", "loop {")
        self.assertIn("awaiting_document()", poll, "the old dom never writes into the new page")

    def test_a_reloaded_document_gets_a_fresh_dom_on_the_old_route(self):
        app = (self.vendor / "app.rs").read_text()
        init = body_of(app, "pub fn handle_initialize_msg(", "view.dom.rebuild")
        self.assertIn("document_initialized()", init)
        self.assertIn("remount_fresh_dom()", init)
        webview = (self.vendor / "webview.rs").read_text()
        remount = body_of(webview, "pub(crate) fn remount_fresh_dom(", "\n    }\n")
        self.assertIn("carry_route(", remount)
        self.assertIn("set_runtime(dom.runtime())", remount, "events go to the new dom")
        launch = (self.vendor / "launch.rs").read_text()
        self.assertIn("crate::recovery::set_dom_factory(", launch)

    def test_new_edit_server_location_is_published_before_webviews_are_told(self):
        edits = (self.vendor / "edits.rs").read_text()
        loop = body_of(edits, "fn accept_loop(", "\n    }\n")
        self.assertLess(loop.index("*current_location.lock().unwrap() = location;"),
                        loop.index("notify.notify_waiters();"))

    def test_app_restarts_page_helpers_for_each_document(self):
        app = (ROOT / "src/app.rs").read_text()
        self.assertIn("use_hook(crate::webview_recovery::reset_document_scoped_state);", app)
        self.assertIn("crate::webview_recovery::use_webview_recovery();", app)
        chart = (ROOT / "src/telemetry_dashboard/data_chart.rs").read_text()
        reset = body_of(chart, "pub(crate) fn reset_for_new_document()", "\n}\n")
        self.assertIn("CHART_CANVAS_RENDERER_INSTALLED.store(false", reset)
        self.assertIn("CHART_DRAW_FLUSH_SCHEDULED.store(false", reset)

    def test_patch_notes_describe_the_recovery_change(self):
        notes = (ROOT / "vendor-dioxus-desktop/GROUNDSTATION-PATCH.md").read_text()
        self.assertIn("web content process", notes)


if __name__ == "__main__":
    unittest.main()

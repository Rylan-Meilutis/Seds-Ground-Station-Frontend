import contextlib
import http.server
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("frontend_ci_webview2", ROOT / "scripts/ci/frontend.py")
ci = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci)


class TruncatingServer:
    """Serves BODY, but the first `cut` responses stop halfway through it."""

    BODY = bytes(range(256)) * 4096

    def __init__(self, cut):
        state = {"left": cut, "requests": 0}
        body = self.BODY

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                state["requests"] += 1
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                if state["left"]:
                    state["left"] -= 1
                    self.wfile.write(body[: len(body) // 2])
                    self.close_connection = True
                else:
                    self.wfile.write(body)

            def log_message(self, *args):
                pass

        self.state = state
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}/installer.exe"

    def __enter__(self):
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        return self

    def __exit__(self, *exc):
        self.server.shutdown()
        self.server.server_close()


class Download(unittest.TestCase):
    def test_a_body_cut_short_is_retried_never_kept(self):
        with TruncatingServer(cut=2) as server, tempfile.TemporaryDirectory() as directory, \
                patch.object(ci.time, "sleep"):
            target = Path(directory) / "installer.exe"
            ci.download(server.url, target, attempts=5)
            self.assertEqual(target.read_bytes(), TruncatingServer.BODY)
            self.assertEqual(server.state["requests"], 3)

    def test_a_body_always_cut_short_fails_and_leaves_no_file(self):
        with TruncatingServer(cut=10) as server, tempfile.TemporaryDirectory() as directory, \
                patch.object(ci.time, "sleep"):
            target = Path(directory) / "installer.exe"
            with self.assertRaises(ci.http.client.HTTPException):
                ci.download(server.url, target, attempts=3)
            self.assertFalse(target.exists())
            self.assertEqual(server.state["requests"], 3)


class WebView2(unittest.TestCase):
    def install(self, signature_exit, signer="CN=Microsoft Corporation, O=Microsoft Corporation"):
        calls = {}

        def fake_download(url, path, **options):
            calls["download"] = (url, options)
            path.write_bytes(b"MZ installer")

        def fake_run(argv, env, capture_output, text, timeout):
            calls["verified"] = env["WEBVIEW2_INSTALLER"]
            self.assertTrue(Path(env["WEBVIEW2_INSTALLER"]).is_file())
            return subprocess.CompletedProcess(argv, signature_exit, signer + "\n", "")

        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        (root / "ci-results").mkdir()
        with patch.object(ci, "ROOT", root), patch.dict(os.environ, {"DX_HOME": str(root / "dx")}), \
                patch.object(ci, "download", side_effect=fake_download), \
                patch.object(ci.subprocess, "run", side_effect=fake_run):
            try:
                ci.install_webview2_offline_installer({"PATH": ""})
                error = None
            except RuntimeError as caught:
                error = caught
        return root, calls, error

    def test_signed_installer_lands_where_dx_looks(self):
        root, calls, error = self.install(0)
        self.assertIsNone(error)
        installed = root / "dx" / "tools" / "MicrosoftEdgeWebView2RuntimeInstaller_x64.exe"
        self.assertEqual(installed.read_bytes(), b"MZ installer")
        url, options = calls["download"]
        self.assertEqual(url, "https://go.microsoft.com/fwlink/?linkid=2099617")
        self.assertGreaterEqual(options["attempts"], 3)
        receipt = json.loads((root / "ci-results" / "webview2-offline-installer.json").read_text())
        self.assertEqual(receipt["bytes"], len(b"MZ installer"))
        self.assertIn("Microsoft Corporation", receipt["signer"])
        # Nothing left beside it but the installer.
        self.assertEqual([p.name for p in (root / "dx" / "tools").iterdir()], [installed.name])

    def test_unsigned_installer_is_refused_and_not_installed(self):
        root, calls, error = self.install(1, signer="NotSigned")
        self.assertIsNotNone(error)
        self.assertIn("not validly signed", str(error))
        self.assertEqual(list((root / "dx" / "tools").iterdir()), [])

    def test_name_and_link_match_dx(self):
        """dx 0.7.10 (bundler/tools.rs) looks for this name and downloads this link."""
        self.assertEqual(ci.WEBVIEW2_OFFLINE_NAME, "MicrosoftEdgeWebView2RuntimeInstaller_x64.exe")
        self.assertEqual(ci.WEBVIEW2_OFFLINE_URL, "https://go.microsoft.com/fwlink/?linkid=2099617")

    def test_only_the_windows_bootstrap_prefetches_it(self):
        for name, expected in (("windows", 1), ("web", 0), ("android", 0)):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory, \
                    patch.object(ci, "ROOT", Path(directory)), \
                    patch.object(ci, "validate_host", return_value=("Windows", "x86_64")), \
                    patch.object(ci, "resolved_tool_versions",
                                 return_value={"dioxus": "0.7.10", "wasm-bindgen": "0.2.130"}), \
                    patch.object(ci, "wasm_bindgen_asset"), patch.object(ci, "fetch_application"), \
                    patch.object(ci, "checked"), patch.object(ci, "install_dioxus_cli"), \
                    patch.object(ci, "install_wasm_bindgen"), patch.object(ci, "install_binaryen"), \
                    patch.object(ci, "install_bundletool"), \
                    patch.object(ci, "install_webview2_offline_installer") as webview2, \
                    patch.object(ci.shutil, "which", return_value="/fixture/tool"), \
                    contextlib.redirect_stdout(io.StringIO()):
                (Path(directory) / "Cargo.lock").write_text("fixture")
                ci.bootstrap(name)
                self.assertEqual(webview2.call_count, expected)


if __name__ == "__main__":
    unittest.main()

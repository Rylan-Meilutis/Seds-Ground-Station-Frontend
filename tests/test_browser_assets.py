import importlib.util
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/browser_assets.py"
spec = importlib.util.spec_from_file_location("browser_assets", SCRIPT)
assets = importlib.util.module_from_spec(spec)
spec.loader.exec_module(assets)
DATA = {
    "maplibre-gl.css": b".maplibregl-map{position:relative;}/*" + b"fixture" * 40 + b"*/",
    "maplibre-gl.js": b"globalThis.maplibregl={};/*" + b"fixture" * 40 + b"*/",
}


class BrowserAssetsTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="browser assets ")
        self.root = Path(self.temporary.name)
        self.cache = self.root / "cache"
        self.requests = []
        self.fail = set()
        self.truncate = set()
        self.delay = None
        self.request_started = threading.Event()
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                name = self.path.rsplit("/", 1)[-1]
                outer.requests.append(self.path)
                outer.request_started.set()
                if outer.delay is not None:
                    outer.delay.wait(5)
                if name in outer.fail:
                    self.send_error(503)
                    return
                if name not in DATA:
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header("Content-Type", "text/css" if name.endswith("css") else "text/javascript")
                self.send_header("Content-Length", str(len(DATA[name])))
                self.end_headers()
                try:
                    self.wfile.write(DATA[name][:len(DATA[name]) // 2] if name in outer.truncate else DATA[name])
                except (BrokenPipeError, ConnectionResetError):
                    pass

            def log_message(self, *args):
                pass

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.base = f"http://127.0.0.1:{self.server.server_port}"

    def tearDown(self):
        if self.delay is not None:
            self.delay.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        self.temporary.cleanup()

    def prepare(self, root=None, version="test-1"):
        return assets.prepare(root or self.root / "ui", cache=self.cache, version=version, base=self.base)

    def process(self, root):
        environment = dict(os.environ, MAPLIBRE_GL_VERSION="test-1", MAPLIBRE_GL_BASE_URL=self.base)
        return subprocess.Popen([sys.executable, str(SCRIPT), "--root", str(root), "--cache", str(self.cache)], env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def valid(self, directory, version="test-1"):
        self.assertEqual(assets.read_valid(directory, version, self.base), DATA)
        self.assertEqual((directory / assets.STAMP).read_text(), f"version={version}\n")

    def test_missing_source_and_placeholder_repair_use_verified_cache(self):
        directory = self.prepare()
        self.valid(directory)
        self.assertEqual(len(self.requests), 2)
        cache = next(path for path in self.cache.iterdir() if path.is_dir())
        (directory / ".browser-asset-interrupted").write_bytes(b"unfinished")
        (cache / ".browser-asset-interrupted").write_bytes(b"unfinished")
        (directory / "maplibre-gl.css").write_text("/* gs26-maplibre-placeholder */" * 20)
        directory = self.prepare()
        self.valid(directory)
        self.assertEqual(len(self.requests), 2, "cache recovery must not use the network")
        with patch.object(assets, "urlopen", side_effect=AssertionError("valid source must work offline")):
            self.prepare()
        self.valid(directory)
        self.assertFalse(list(directory.glob(".browser-asset-*")))
        self.assertFalse(list(cache.glob(".browser-asset-*")))

    def test_corrupt_cache_and_malformed_receipt_are_not_trusted(self):
        first = self.prepare()
        cache = next(path for path in self.cache.iterdir() if path.is_dir())
        (cache / "maplibre-gl.js").write_bytes(b"corrupt")
        second = self.prepare(self.root / "second")
        self.valid(first)
        self.valid(second)
        self.assertEqual(len(self.requests), 4)
        (second / assets.RECEIPT).write_text('{"format":1,"version":"test-1","base_url":' + json.dumps(self.base) + ',"assets":[]}')
        self.assertIsNone(assets.read_valid(second, "test-1", self.base))
        self.prepare(self.root / "second")
        self.valid(second)
        self.assertEqual(len(self.requests), 4)

    def test_parallel_processes_share_cache_and_source_lock(self):
        roots = [self.root / "one", self.root / "one", self.root / "two", self.root / "three"]
        processes = [self.process(root) for root in roots]
        for process in processes:
            output, error = process.communicate(timeout=15)
            self.assertEqual(process.returncode, 0, error.decode())
            self.assertIn(b"verified", output)
        self.assertEqual(len(self.requests), 2, "parallel cold preparation downloads each file once")
        for root in roots:
            self.valid(root / "static/vendor/maplibre-gl")

    def test_failure_preserves_previous_pair_and_does_not_publish_new_stamp(self):
        directory = self.prepare()
        before = {path.name: path.read_bytes() for path in directory.iterdir() if path.name != ".prepare.lock"}
        self.fail.add("maplibre-gl.js")
        with self.assertRaisesRegex(RuntimeError, "Unable to prepare maplibre-gl.js"):
            self.prepare(version="test-2")
        self.assertEqual({path.name: path.read_bytes() for path in directory.iterdir() if path.name != ".prepare.lock"}, before)
        self.valid(directory)
        self.assertEqual(sum(path.endswith("maplibre-gl.js") and "test-2" in path for path in self.requests), 3)

    def test_version_override_refreshes_pair_before_updating_stamp(self):
        directory = self.prepare()
        self.prepare(version="test-2")
        self.valid(directory, version="test-2")
        self.assertEqual(len(self.requests), 4)
        self.prepare(version="test-2")
        self.assertEqual(len(self.requests), 4)

    def test_truncated_body_is_not_cached_or_published(self):
        self.truncate.add("maplibre-gl.js")
        with self.assertRaisesRegex(RuntimeError, "Incomplete response|IncompleteRead"):
            self.prepare()
        directory = self.root / "ui/static/vendor/maplibre-gl"
        self.assertFalse((directory / assets.RECEIPT).exists())
        self.assertFalse((directory / "maplibre-gl.js").exists())

    def test_pinned_version_rejects_changed_upstream_bytes(self):
        with self.assertRaisesRegex(RuntimeError, "Checksum mismatch"):
            self.prepare(version=assets.DEFAULT_VERSION)
        directory = self.root / "ui/static/vendor/maplibre-gl"
        self.assertFalse((directory / assets.RECEIPT).exists())
        self.assertFalse((directory / assets.STAMP).exists())

    def test_atomic_failure_retains_old_file_and_cleans_temporary(self):
        path = self.root / "old.js"
        path.write_bytes(b"old")
        with patch.object(assets.os, "replace", side_effect=OSError("fake disk failure")):
            with self.assertRaisesRegex(OSError, "fake disk failure"):
                assets.atomic_write(path, b"new")
        self.assertEqual(path.read_bytes(), b"old")
        self.assertFalse(list(self.root.glob(".browser-asset-*")))

    def test_interrupted_process_releases_source_and_cache_locks(self):
        self.delay = threading.Event()
        root = self.root / "interrupted"
        process = self.process(root)
        self.assertTrue(self.request_started.wait(5))
        process.kill()
        process.communicate(timeout=5)
        self.delay.set()
        result = self.process(root)
        _, error = result.communicate(timeout=15)
        self.assertEqual(result.returncode, 0, error.decode())
        self.valid(root / "static/vendor/maplibre-gl")

    def test_frontend_build_flag_needs_no_rust_or_ui_bootstrap(self):
        root = self.root / "flag source"
        environment = dict(os.environ, PATH="", MAPLIBRE_GL_VERSION="test-1", MAPLIBRE_GL_BASE_URL=self.base)
        process = subprocess.run([sys.executable, str(SCRIPT.parents[1] / "build.py"), "--prepare-browser-assets", "--root", str(root), "--cache", str(self.cache)], env=environment, capture_output=True, timeout=15)
        self.assertEqual(process.returncode, 0, process.stderr.decode())
        self.assertIn(b"verified", process.stdout)
        self.valid(root / "static/vendor/maplibre-gl")

    def test_configuration_rejects_credentials_insecure_network_and_path_versions(self):
        for base in ["https://user:secret@example.test", "https://example.test/?token=secret", "http://example.test"]:
            with self.assertRaises(ValueError):
                assets.prepare(self.root / "bad", version="test-1", base=base)
        with self.assertRaises(ValueError):
            self.prepare(version="../../bad")
        self.assertEqual(self.requests, [])


if __name__ == "__main__":
    unittest.main()

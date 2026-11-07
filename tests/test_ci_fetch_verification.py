"""Fetch diagnostic handling with mocked processes; no network/cache/build use."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("ci_fetch_driver", ROOT / "scripts/ci/frontend.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)

SYSCALL = "warning: spurious network error (3 tries remaining): [55] Failed sending data to the peer (OpenSSL SSL_read: SSL_ERROR_SYSCALL, errno 0)"
EOF = "warning: spurious network error (3 tries remaining): [55] Failed sending data to the peer (OpenSSL SSL_read: OpenSSL/3.6.3: error:0A000126:SSL routines::unexpected eof while reading, errno 0)"


class FetchVerification(unittest.TestCase):
    def run_fetch(self, rows, *, fails=False):
        children = [Mock(stdout=io.StringIO(text), wait=Mock(return_value=code)) for text, code in rows]
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
             patch.object(ci.shutil, "which", return_value="/prepared/cargo"), \
             patch.object(ci.subprocess, "Popen", side_effect=children) as spawn, \
             contextlib.redirect_stdout(io.StringIO()):
            if fails:
                with self.assertRaises(ci.CommandFailure):
                    ci.fetch_application(os.environ.copy())
            else:
                ci.fetch_application(os.environ.copy())
            logs = Path(directory) / "ci-results"
            receipts = {p.stem: json.loads(p.read_text()) for p in logs.glob("*.json")}
            raw = {p.stem: p.read_text() for p in logs.glob("*.log")}
            return [call.args[0] for call in spawn.call_args_list], receipts, raw

    def test_clean_fetch_needs_no_verification(self):
        calls, receipts, _ = self.run_fetch([("Downloaded fixture\n", 0)])
        self.assertEqual(calls, [["/prepared/cargo", "fetch", "--locked"]])
        self.assertEqual(receipts["application-fetch"]["warning_lines"], 0)

    def test_observed_ssl_warnings_require_one_clean_locked_offline_fetch(self):
        text = EOF + "\n" + (SYSCALL + "\n") * 98
        calls, receipts, raw = self.run_fetch([(text, 0), ("", 0)])
        self.assertEqual(calls, [["/prepared/cargo", "fetch", "--locked"],
                                 ["/prepared/cargo", "fetch", "--locked", "--offline"]])
        self.assertEqual(raw["application-fetch"], text)
        self.assertEqual(receipts["application-fetch"]["warning_lines"], 99)
        self.assertEqual(receipts["application-fetch"]["exit"], 0)
        self.assertEqual(receipts["application-fetch-offline-verification"]["warning_lines"], 0)

    def test_mixed_compiler_and_deprecation_warnings_never_retry(self):
        for warning in ["warning: unused import: fixture", "warning[deprecated]: fixture", "warning: use of deprecated function"]:
            with self.subTest(warning=warning):
                calls, _, _ = self.run_fetch([(SYSCALL + "\n" + warning + "\n", 0)], fails=True)
                self.assertEqual(len(calls), 1)

    def test_permanent_unknown_and_near_match_warnings_never_retry(self):
        for warning in ["warning: permanent fetch configuration error", SYSCALL + " extra",
                        SYSCALL.replace("[55]", "[60]"), SYSCALL.replace("errno 0", "errno 5")]:
            with self.subTest(warning=warning):
                calls, _, _ = self.run_fetch([(warning + "\n", 0)], fails=True)
                self.assertEqual(len(calls), 1)

    def test_failed_initial_fetch_never_retries_even_with_recognized_warnings(self):
        for text in ["", SYSCALL + "\n"]:
            with self.subTest(text=text):
                calls, receipts, _ = self.run_fetch([(text, 101)], fails=True)
                self.assertEqual(len(calls), 1)
                self.assertEqual(receipts["application-fetch"]["exit"], 101)

    def test_offline_warning_or_exit_failure_stops_without_third_attempt(self):
        for text, code in [(SYSCALL + "\n", 0), ("warning: deprecated configuration\n", 0),
                           ("error: cache incomplete\n", 101)]:
            with self.subTest(text=text, code=code):
                calls, receipts, _ = self.run_fetch([(SYSCALL + "\n", 0), (text, code)], fails=True)
                self.assertEqual(len(calls), 2)
                self.assertEqual(receipts["application-fetch-offline-verification"]["exit"], code)

    def test_other_checked_commands_still_reject_identical_transport_warning(self):
        child = Mock(stdout=io.StringIO(SYSCALL + "\n"), wait=Mock(return_value=0))
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
             patch.object(ci.subprocess, "Popen", return_value=child), \
             contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(ci.CommandFailure):
                ci.checked(["/prepared/compiler"], {}, "compiler")


if __name__ == "__main__":
    unittest.main()

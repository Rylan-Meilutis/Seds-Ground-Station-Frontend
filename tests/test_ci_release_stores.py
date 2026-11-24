"""Release driver contracts for the protected-ref guard and the store uploads
(TestFlight and the Play internal track). Nothing here contacts Apple, Google
or GitHub; uploads are exercised against recorded calls."""
import base64
import contextlib
import datetime
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("frontend_ci_stores", ROOT / "scripts/ci/frontend.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)

GIT_ENV = {"GIT_AUTHOR_NAME": "ci", "GIT_AUTHOR_EMAIL": "ci@example.invalid", "GIT_COMMITTER_NAME": "ci",
           "GIT_COMMITTER_EMAIL": "ci@example.invalid", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}


def git(cwd, *args):
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True,
                          env={**os.environ, **GIT_ENV}).stdout.strip()


class ReleaseGuard(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        base = Path(self.temp.name)
        origin, work = base / "origin.git", base / "work"
        git(base, "init", "--bare", "-b", "main", str(origin))
        git(base, "clone", str(origin), str(work))
        git(work, "commit", "--allow-empty", "-m", "main")
        git(work, "push", "origin", "HEAD:main")
        git(work, "checkout", "-b", "dev")
        git(work, "commit", "--allow-empty", "-m", "dev")
        git(work, "push", "origin", "dev")
        git(work, "tag", "v0.3.1b36")
        git(work, "checkout", "-b", "feature")
        git(work, "commit", "--allow-empty", "-m", "unreviewed")
        git(work, "push", "origin", "feature")
        git(work, "tag", "v0.3.1b37")
        # A release checkout knows only the tag; origin/* comes from the guard's fetch.
        for branch in ["main", "dev", "feature"]:
            git(work, "update-ref", "-d", f"refs/remotes/origin/{branch}")
        self.work = work

    def tearDown(self):
        self.temp.cleanup()

    def guard(self, tag, event="push", ref=None, protected="true", dry_run=None, sha=None):
        env = {"GITHUB_EVENT_NAME": event, "GITHUB_REF": ref or f"refs/tags/{tag}", "GITHUB_REF_PROTECTED": protected,
               "RELEASE_DRY_RUN": dry_run or "false", "GITHUB_SHA": sha or "", **GIT_ENV}
        with patch.object(ci, "ROOT", self.work), patch.dict(os.environ, env), \
             contextlib.redirect_stdout(io.StringIO()):
            return ci.release_guard(tag)

    def test_protected_tag_on_dev_passes(self):
        result = self.guard("v0.3.1b36")
        self.assertEqual((result["guarded"], result["release_branch"]), ("true", "dev"))
        self.assertEqual(result["commit"], git(self.work, "rev-parse", "v0.3.1b36^{commit}"))

    def test_dispatch_from_protected_release_branch_passes(self):
        self.assertEqual(self.guard("v0.3.1b36", "workflow_dispatch", "refs/heads/main")["guarded"], "true")

    def test_tag_runs_check_out_the_tag(self):
        self.assertEqual(self.guard("v0.3.1b36")["ref"], "refs/tags/v0.3.1b36")

    def test_dispatched_dry_run_builds_the_protected_branch_head_without_a_tag(self):
        dev_head = git(self.work, "rev-parse", "v0.3.1b36^{commit}")   # dev's head in this fixture
        result = self.guard("v0.3.1b40", "workflow_dispatch", "refs/heads/dev", dry_run="true", sha=dev_head)
        self.assertEqual((result["guarded"], result["release_branch"], result["commit"], result["ref"]),
                         ("true", "dev", dev_head, dev_head))

    def test_dry_run_still_refuses_unprotected_or_feature_commits(self):
        feature = git(self.work, "rev-parse", "v0.3.1b37^{commit}")
        with self.assertRaisesRegex(RuntimeError, "not a protected ref"):
            self.guard("v0.3.1b40", "workflow_dispatch", "refs/heads/dev", protected="false", dry_run="true", sha=feature)
        with self.assertRaisesRegex(RuntimeError, "not on"):
            self.guard("v0.3.1b40", "workflow_dispatch", "refs/heads/dev", dry_run="true", sha=feature)
        with self.assertRaisesRegex(RuntimeError, "from main or dev"):
            self.guard("v0.3.1b40", "workflow_dispatch", "refs/heads/feature", dry_run="true", sha=feature)

    def test_dry_run_flag_is_ignored_on_tag_pushes(self):
        # A pushed tag always builds the tag, never a branch head.
        self.assertEqual(self.guard("v0.3.1b36", dry_run="true", sha="0" * 40)["ref"], "refs/tags/v0.3.1b36")

    def test_refusals(self):
        cases = [
            ("v0.3.1b37", {}, "not on"),                                          # tag on a feature branch
            ("v0.3.1b36", {"protected": "false"}, "not a protected ref"),
            ("v0.3.1b36", {"protected": ""}, "not a protected ref"),
            ("v0.3.1b36", {"event": "workflow_dispatch", "ref": "refs/heads/feature"}, "from main or dev"),
            ("v0.3.1b36", {"event": "pull_request"}, "only from a tag push"),
            ("v0.3.1b36", {"ref": "refs/tags/v0.3.1b37"}, "arrived with ref"),
            ("v0.3.1", {}, "Not a release tag"),
        ]
        for tag, kwargs, message in cases:
            with self.subTest(tag=tag, **kwargs), self.assertRaisesRegex(RuntimeError, message):
                self.guard(tag, **kwargs)


class StoreCheck(unittest.TestCase):
    def check(self, store, values):
        with tempfile.NamedTemporaryFile() as output, \
             patch.dict(os.environ, {**values, "GITHUB_OUTPUT": output.name}, clear=True), \
             contextlib.redirect_stdout(io.StringIO()) as stdout:
            enabled = ci.store_check(store)
            return enabled, Path(output.name).read_text(), stdout.getvalue()

    def test_unset_store_secrets_skip_with_a_notice(self):
        android = {"ANDROID_UPLOAD_KEYSTORE_B64": "a", "ANDROID_KEYSTORE_PASSWORD": "b"}
        notary = {name: "x" for name in ci.NOTARY_API_SECRETS}
        for store, values in [("testflight", {}), ("testflight", notary), ("play", {}), ("play", android)]:
            with self.subTest(store=store, values=sorted(values)):
                enabled, output, stdout = self.check(store, values)
                self.assertFalse(enabled)
                self.assertEqual(output, "enabled=false\n")
                self.assertIn("::notice title=", stdout)
                self.assertIn("skipped", stdout)

    def test_complete_secrets_enable_and_partial_sets_fail(self):
        testflight = {name: "x" for name in ci.IOS_STORE_SECRETS + ci.NOTARY_API_SECRETS}
        play = {name: "x" for name in ci.PLAY_STORE_SECRETS + ci.ANDROID_RELEASE_SECRETS}
        for store, values in [("testflight", testflight), ("play", play)]:
            enabled, output, _ = self.check(store, values)
            self.assertTrue(enabled)
            self.assertEqual(output, "enabled=true\n")
        with self.assertRaisesRegex(RuntimeError, "APPLE_NOTARY_KEY_P8_B64"):
            self.check("testflight", {name: "x" for name in ci.IOS_STORE_SECRETS})
        with self.assertRaisesRegex(RuntimeError, "IOS_APPSTORE_PROFILE_B64"):
            self.check("testflight", {"IOS_DISTRIBUTION_P12_B64": "x"})
        with self.assertRaisesRegex(RuntimeError, "ANDROID_UPLOAD_KEYSTORE_B64"):
            self.check("play", {"PLAY_SERVICE_ACCOUNT_JSON_B64": "x"})


class AppStoreProfile(unittest.TestCase):
    NOW = datetime.datetime(2026, 11, 24)

    def profile(self, **changes):
        info = {"TeamIdentifier": ["TEAM123456"], "Name": "GS26 App Store",
                "ExpirationDate": datetime.datetime(2027, 11, 24),
                "Entitlements": {"application-identifier": "TEAM123456.com.UBSEDS.GS26", "get-task-allow": False}}
        info.update(changes)
        return info

    def test_app_store_profile_for_the_bundle_is_accepted(self):
        self.assertEqual(ci.check_appstore_profile(self.profile(), now=self.NOW), "TEAM123456")

    def test_wrong_kind_bundle_or_expired_profiles_are_refused(self):
        for changes, message in [
                ({"ProvisionedDevices": ["00008110-0001"]}, "development, ad hoc"),
                ({"ProvisionsAllDevices": True}, "enterprise"),
                ({"Entitlements": {"application-identifier": "TEAM123456.com.UBSEDS.GS26", "get-task-allow": True}},
                 "get-task-allow"),
                ({"Entitlements": {"application-identifier": "TEAM123456.com.UBSEDS.GS26.watchkitapp"}}, "not a profile"),
                ({"Entitlements": {"application-identifier": "OTHER.com.UBSEDS.GS26"}}, "not a profile"),
                ({"ExpirationDate": datetime.datetime(2026, 11, 1)}, "expired")]:
            with self.subTest(changes=changes), self.assertRaisesRegex(RuntimeError, message):
                ci.check_appstore_profile(self.profile(**changes), now=self.NOW)


class StoreBuild(unittest.TestCase):
    def test_store_commands_build_signed_ipa_and_aab(self):
        self.assertEqual(ci.store_command("ios")[1:], ["build.py", "--target", "ios", "--release", "--final"])
        self.assertEqual(ci.store_command("android")[1:],
                         ["build.py", "--target", "android", "--release", "--final", "--package-type", "aab"])
        for name in ["watchos", "android-armv7", "macos"]:
            with self.assertRaises(ValueError):
                ci.store_command(name)

    def test_ios_build_env_gets_only_the_job_local_profile(self):
        with tempfile.TemporaryDirectory() as temp:
            profile = Path(temp) / "seds-release-signing" / "appstore.mobileprovision"
            values = {"RUNNER_TEMP": temp, "IOS_DISTRIBUTION_MOBILEPROVISION": str(profile),
                      "IOS_DISTRIBUTION_P12_B64": "fixture", "IOS_DISTRIBUTION_P12_PASSWORD": "fixture",
                      "IOS_APPSTORE_PROFILE_B64": "fixture", "APPLE_NOTARY_KEY_P8_B64": "fixture",
                      "PLAY_SERVICE_ACCOUNT_JSON_B64": "fixture"}
            with patch.dict(os.environ, values):
                env = ci.release_env("ios")
            self.assertEqual(env["IOS_DISTRIBUTION_MOBILEPROVISION"], str(profile))
            for name in ["IOS_DISTRIBUTION_P12_B64", "IOS_DISTRIBUTION_P12_PASSWORD", "IOS_APPSTORE_PROFILE_B64",
                         "APPLE_NOTARY_KEY_P8_B64"]:
                self.assertNotIn(name, env)
            with patch.dict(os.environ, {**values, "IOS_DISTRIBUTION_MOBILEPROVISION": "/elsewhere/p.mobileprovision"}), \
                 self.assertRaisesRegex(RuntimeError, "job-local"):
                ci.release_env("ios")
            with patch.dict(os.environ, {"RUNNER_TEMP": temp}, clear=True), \
                 self.assertRaisesRegex(RuntimeError, "not prepared"):
                ci.release_env("ios")

    def test_build_only_env_drops_store_secrets(self):
        names = ["IOS_DISTRIBUTION_P12_B64", "IOS_DISTRIBUTION_P12_PASSWORD", "IOS_APPSTORE_PROFILE_B64",
                 "PLAY_SERVICE_ACCOUNT_JSON_B64", "IOS_DISTRIBUTION_MOBILEPROVISION"]
        with patch.dict(os.environ, {name: "fixture" for name in names}):
            env = ci.ci_env()
        self.assertFalse(set(names) & set(env))

    def test_jar_signer_fingerprints(self):
        digest = "41:BE:92:37:28:08:D8:9E:C0:01:3D:89:B5:2E:20:3D:8D:5A:A6:FD:0A:A7:F8:40:38:39:29:BC:D7:87:8F:0E"
        output = ("Signer #1:\n\nSignature:\n\nOwner: CN=Upload\nCertificate fingerprints:\n"
                  "\t SHA1: 2D:00:C6:60:5A:80:8E:F6:06:7E:F5:11:62:63:58:F7:22:DF:4B:3C\n"
                  f"\t SHA256: {digest}\n")
        self.assertEqual(ci.jar_signer_sha256(output), [ci.normalized_digest(digest)])


class SigningExpiry(unittest.TestCase):
    def certificate(self, days):
        with tempfile.TemporaryDirectory() as temp:
            subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", str(days),
                            "-subj", "/CN=fixture", "-keyout", f"{temp}/k.pem", "-out", f"{temp}/c.pem",
                            "-outform", "DER"], check=True, capture_output=True)
            return Path(temp, "c.pem").read_bytes()

    def report(self, label, expires, now=None):
        with tempfile.NamedTemporaryFile() as summary, patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": summary.name}), \
             contextlib.redirect_stdout(io.StringIO()) as stdout:
            days = ci.report_expiry(label, expires, now)
            return days, stdout.getvalue(), Path(summary.name).read_text()

    def test_certificate_expiry_is_read_and_reported(self):
        expires = ci.cert_not_after(self.certificate(400))
        self.assertAlmostEqual((expires - ci.utc_now()).days, 399, delta=1)
        days, stdout, summary = self.report("Android upload certificate", expires)
        self.assertGreater(days, 30)
        self.assertNotIn("::warning", stdout)
        self.assertIn(f"Android upload certificate: valid until {expires:%Y-%m-%d}", summary)
        self.assertIsNone(ci.cert_not_after(b"not a certificate"))

    def test_under_thirty_days_unknown_or_expired_warn_without_failing(self):
        now = datetime.datetime(2027, 1, 10)
        for expires, needle in [(datetime.datetime(2027, 2, 4), "expires in 25 days"),
                                (datetime.datetime(2027, 1, 1), "EXPIRED"),
                                (None, "Could not read")]:
            with self.subTest(expires=expires):
                _days, stdout, _summary = self.report("Apple Distribution certificate", expires, now)
                self.assertIn("::warning title=Signing expiry::", stdout)
                self.assertIn(needle, stdout)
        _days, stdout, _ = self.report("App Store provisioning profile", datetime.datetime(2027, 2, 4),
                                       datetime.datetime(2026, 11, 24))
        self.assertNotIn("::warning", stdout)


INFO = {"tag": "v0.3.1b36", "version": "0.3.1", "build": "36", "name": "Version 0.3.1 Build 36",
        "version_code": 301036}


class PlayUpload(unittest.TestCase):
    def run_upload(self, responses):
        """responses: {(method, url suffix): result or ci.PlayError} -> (outcome, calls)."""
        calls = []
        def call(token, method, url, *, body=None, data=None, content_type="application/json", timeout=120):
            calls.append((method, url, body))
            for (want_method, suffix), result in responses.items():
                if method == want_method and url.endswith(suffix):
                    if isinstance(result, list):
                        result = result.pop(0)
                    if isinstance(result, Exception):
                        raise result
                    return result
            return {}
        account = {"client_email": "uploader@example.iam.gserviceaccount.com", "private_key": "PEM",
                   "token_uri": ci.GOOGLE_TOKEN_URI}
        with tempfile.TemporaryDirectory() as temp, tempfile.TemporaryDirectory() as runner:
            root = Path(temp)
            aab = root / ci.STORE_OUTPUTS["android"]
            aab.parent.mkdir(parents=True)
            aab.write_bytes(b"aab")
            (Path(runner) / "seds-release-signing").mkdir()
            keys = []
            def token(account_arg, key_path):
                keys.append((key_path, key_path.read_bytes()))
                return "fixture-token"
            env = {"RUNNER_TEMP": runner,
                   "PLAY_SERVICE_ACCOUNT_JSON_B64": base64.b64encode(json.dumps(account).encode()).decode()}
            with patch.object(ci, "ROOT", root), patch.dict(os.environ, env, clear=True), \
                 patch.object(ci, "google_access_token", side_effect=token), \
                 contextlib.redirect_stdout(io.StringIO()):
                try:
                    outcome = ci.upload_play(INFO, call=call)
                except RuntimeError as error:
                    outcome = error
            self.assertEqual(keys[0][1], b"PEM")
            self.assertFalse(keys[0][0].exists(), "service-account key removed right after use")
            self.assertTrue(str(keys[0][0]).startswith(runner))
        return outcome, calls

    def tracks(self, calls):
        return [body for method, url, body in calls if method == "PUT"]

    def test_uploads_only_to_the_internal_track_and_commits(self):
        outcome, calls = self.run_upload({("POST", "/edits"): {"id": "e1"},
                                          ("POST", "bundles?uploadType=media"): {"versionCode": 301036}})
        self.assertEqual(outcome, "uploaded")
        self.assertEqual(ci.PLAY_TRACK, "internal")
        self.assertTrue(calls[1][1].startswith(ci.PLAY_UPLOAD_API + "/com.UBSEDS.GS26/edits/e1/bundles"))
        self.assertEqual(self.tracks(calls), [{"track": "internal", "releases": [
            {"name": "Version 0.3.1 Build 36", "versionCodes": ["301036"], "status": "completed"}]}])
        self.assertTrue(calls[2][1].endswith("/edits/e1/tracks/internal"))
        self.assertEqual(calls[-1][:2], ("POST", ci.PLAY_API + "/com.UBSEDS.GS26/edits/e1:commit"))
        self.assertNotIn("DELETE", [method for method, _, _ in calls])

    def test_never_published_app_gets_a_draft_internal_release(self):
        draft = ci.PlayError(400, "Only releases with status draft may be created on draft app.")
        outcome, calls = self.run_upload({("POST", "/edits"): {"id": "e1"},
                                          ("POST", "bundles?uploadType=media"): {"versionCode": 301036},
                                          ("POST", ":commit"): [draft, {}]})
        self.assertEqual(outcome, "uploaded")
        self.assertEqual([t["releases"][0]["status"] for t in self.tracks(calls)], ["completed", "draft"])
        self.assertEqual({t["track"] for t in self.tracks(calls)}, {"internal"})

    def test_rerun_with_the_version_already_on_internal_is_a_no_op(self):
        used = ci.PlayError(403, "APK specifies a version code that has already been used.")
        outcome, calls = self.run_upload({("POST", "/edits"): {"id": "e1"},
                                          ("POST", "bundles?uploadType=media"): used,
                                          ("GET", "/tracks/internal"): {"releases": [{"versionCodes": ["301036"]}]}})
        self.assertEqual(outcome, "already-uploaded")
        self.assertEqual(self.tracks(calls), [])
        self.assertEqual(calls[-1][0], "DELETE")

    def test_used_version_code_elsewhere_and_other_errors_fail_and_abandon_the_edit(self):
        used = ci.PlayError(403, "APK specifies a version code that has already been used.")
        for responses in [{("POST", "bundles?uploadType=media"): used, ("GET", "/tracks/internal"): {"releases": []}},
                          {("POST", "bundles?uploadType=media"): ci.PlayError(403, "The caller does not have permission")},
                          {("POST", "bundles?uploadType=media"): {"versionCode": 301035}},
                          {("POST", "bundles?uploadType=media"): {"versionCode": 301036},
                           ("POST", ":commit"): ci.PlayError(400, "Something else")}]:
            with self.subTest(responses=list(responses)):
                outcome, calls = self.run_upload({("POST", "/edits"): {"id": "e1"}, **responses})
                self.assertIsInstance(outcome, RuntimeError)
                self.assertEqual(calls[-1][0], "DELETE")


class TestFlightUpload(unittest.TestCase):
    def run_upload(self, result):
        key_bytes = b"-----BEGIN PRIVATE KEY-----fixture"
        seen = []
        def run(argv, **kwargs):
            keys = Path(kwargs["env"]["API_PRIVATE_KEYS_DIR"])
            seen.append((argv, kwargs, (keys / "AuthKey_ABC123XYZ9.p8").read_bytes()))
            return result
        with tempfile.TemporaryDirectory() as runner, tempfile.TemporaryDirectory() as temp:
            (Path(runner) / "seds-release-signing").mkdir()
            env = {"RUNNER_TEMP": runner, "APPLE_NOTARY_KEY_P8_B64": base64.b64encode(key_bytes).decode(),
                   "APPLE_NOTARY_KEY_ID": "ABC123XYZ9", "APPLE_NOTARY_ISSUER_ID": "69a6de70-0000-47e3-e053-5b8c7c11a4d1"}
            with patch.object(ci, "ROOT", Path(temp)), patch.dict(os.environ, env, clear=True), \
                 patch.object(ci.subprocess, "run", side_effect=run), contextlib.redirect_stdout(io.StringIO()):
                try:
                    outcome = ci.upload_testflight(INFO)
                except RuntimeError as error:
                    outcome = error
            home = Path(runner) / "seds-release-signing/appstoreconnect-home"
            self.assertFalse((home / ".appstoreconnect/private_keys/AuthKey_ABC123XYZ9.p8").exists())
        argv, kwargs, written = seen[0]
        self.assertEqual(written, key_bytes)
        self.assertEqual(argv[:4], ["xcrun", "altool", "--upload-app", "--type"])
        self.assertIn("--apiKey", argv)
        self.assertNotIn(key_bytes.decode(), " ".join(argv))
        self.assertEqual(kwargs["env"]["HOME"], str(home))
        self.assertEqual(kwargs["cwd"], home)
        self.assertNotIn("APPLE_NOTARY_KEY_P8_B64", kwargs["env"])
        return outcome

    def test_upload_uses_the_api_key_from_runner_temp_and_removes_it(self):
        self.assertEqual(self.run_upload(subprocess.CompletedProcess([], 0, stdout="{}", stderr="")), "uploaded")

    def test_redundant_upload_is_a_no_op_and_other_failures_fail(self):
        redundant = subprocess.CompletedProcess([], 1, stdout="", stderr="ERROR: Redundant Binary Upload. You've "
                                                "already uploaded a build with build number '36'")
        self.assertEqual(self.run_upload(redundant), "already-uploaded")
        failed = subprocess.CompletedProcess([], 1, stdout="", stderr="ERROR: Authentication failed")
        self.assertIsInstance(self.run_upload(failed), RuntimeError)


if __name__ == "__main__":
    unittest.main()

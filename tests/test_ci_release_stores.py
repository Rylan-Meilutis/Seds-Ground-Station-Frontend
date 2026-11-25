"""Release driver contracts for the protected-ref guard and the store uploads
(TestFlight with optional beta groups, and a Play testing track). Nothing here contacts Apple, Google
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
        for store, values, extra in [("testflight", testflight, "groups=\n"), ("play", play, "track=internal\n")]:
            enabled, output, _ = self.check(store, values)
            self.assertTrue(enabled)
            self.assertEqual(output, "enabled=true\n" + extra)
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


class StoreOptions(unittest.TestCase):
    def test_play_track_defaults_to_internal_and_allows_only_testing_tracks(self):
        self.assertEqual(ci.PLAY_TRACKS, ("internal", "alpha", "beta"))
        for value, expected in [("", "internal"), (None, "internal"), ("  ", "internal"), ("internal", "internal"),
                                ("alpha", "alpha"), ("beta", "beta"), (" Beta ", "beta")]:
            with self.subTest(value=value), patch.dict(os.environ, {}, clear=True):
                self.assertEqual(ci.play_track(value), expected)
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(ci.play_track(), "internal")
        with patch.dict(os.environ, {"PLAY_TRACK": "alpha"}, clear=True):
            self.assertEqual(ci.play_track(), "alpha")

    def test_production_and_unknown_tracks_are_refused(self):
        with self.assertRaisesRegex(RuntimeError, "production is refused"):
            ci.play_track("production")
        with self.assertRaisesRegex(RuntimeError, "production is refused"):
            ci.play_track(" PRODUCTION ")
        for value in ["qa", "internal,alpha", "rollout", "wear:internal", "beta "+"x"]:
            with self.subTest(value=value), self.assertRaisesRegex(RuntimeError, "must be one of"):
                ci.play_track(value)
        with self.assertRaisesRegex(RuntimeError, "Refusing Play track"):
            ci.play_release(301036, "n", "completed", "production")

    def test_store_check_refuses_a_bad_track_even_before_the_skip(self):
        for env in [{"PLAY_TRACK": "production"}, {"PLAY_TRACK": "production", "PLAY_SERVICE_ACCOUNT_JSON_B64": "x"}]:
            with self.subTest(env=sorted(env)), patch.dict(os.environ, env, clear=True), \
                 contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(RuntimeError, "production"):
                ci.store_check("play")
        with tempfile.NamedTemporaryFile() as output, contextlib.redirect_stdout(io.StringIO()), \
             patch.dict(os.environ, {"PLAY_TRACK": "beta", "PLAY_SERVICE_ACCOUNT_JSON_B64": "x",
                                     "GITHUB_OUTPUT": output.name}, clear=True):
            self.assertTrue(ci.store_check("play-promote"))
            self.assertEqual(Path(output.name).read_text(), "enabled=true\ntrack=beta\n")

    def test_testflight_groups_parse(self):
        for value, expected in [("", []), (None, []), ("none", []), (" None ", []),
                                ("External Testers", ["External Testers"]),
                                ("Team, External Testers ,Team,", ["Team", "External Testers"])]:
            with self.subTest(value=value), patch.dict(os.environ, {}, clear=True):
                self.assertEqual(ci.testflight_groups(value), expected)
        for value in ["ok,bad\nname", "x" * 101, ",".join(f"g{i}" for i in range(21))]:
            with self.subTest(value=value[:20]), self.assertRaises(RuntimeError):
                ci.testflight_groups(value)
        with tempfile.NamedTemporaryFile() as output, contextlib.redirect_stdout(io.StringIO()), \
             patch.dict(os.environ, {"TESTFLIGHT_GROUPS": "A, B", "GITHUB_OUTPUT": output.name,
                                     **{n: "x" for n in ci.NOTARY_API_SECRETS}}, clear=True):
            self.assertTrue(ci.store_check("testflight-promote"))
            self.assertEqual(Path(output.name).read_text(), "enabled=true\ngroups=A,B\n")


def recorded(responses, calls):
    """A fake API call: responses {(method, url/path suffix): result | [results] | Exception}."""
    def lookup(method, url):
        for (want_method, suffix), result in responses.items():
            if method == want_method and url.endswith(suffix):
                if isinstance(result, list):
                    result = result.pop(0)
                if isinstance(result, Exception):
                    raise result
                return result
        return {}
    return lookup


class PlayTracks(unittest.TestCase):
    def run_play(self, fn, responses, notes=None):
        calls = []
        lookup = recorded(responses, calls)
        def call(token, method, url, *, body=None, data=None, content_type="application/json", timeout=120):
            calls.append((method, url, body))
            return lookup(method, url)
        with tempfile.TemporaryDirectory() as temp, tempfile.TemporaryDirectory() as runner:
            root = Path(temp)
            aab = root / ci.STORE_OUTPUTS["android"]
            aab.parent.mkdir(parents=True)
            aab.write_bytes(b"aab")
            if notes is not None:
                (root / "docs/release-notes").mkdir(parents=True)
                (root / "docs/release-notes/v0.3.1b36.md").write_text(notes)
            account = {"client_email": "u@example.iam.gserviceaccount.com", "private_key": "PEM",
                       "token_uri": ci.GOOGLE_TOKEN_URI}
            env = {"RUNNER_TEMP": runner,
                   "PLAY_SERVICE_ACCOUNT_JSON_B64": base64.b64encode(json.dumps(account).encode()).decode()}
            with patch.object(ci, "ROOT", root), patch.dict(os.environ, env, clear=True), \
                 patch.object(ci, "google_access_token", return_value="fixture-token"), \
                 contextlib.redirect_stdout(io.StringIO()):
                try:
                    outcome = fn(call)
                except RuntimeError as error:
                    outcome = error
        return outcome, calls

    @staticmethod
    def puts(calls):
        return [(url.rsplit("/", 1)[1], body) for method, url, body in calls if method == "PUT"]

    def test_upload_goes_to_the_selected_track_with_release_notes(self):
        responses = {("POST", "/edits"): {"id": "e1"}, ("POST", "bundles?uploadType=media"): {"versionCode": 301036}}
        outcome, calls = self.run_play(lambda call: ci.upload_play(INFO, call=call, track="alpha"), responses,
                                       notes="Fixes\n" + "x" * 600)
        self.assertEqual(outcome, "uploaded")
        [(track, body)] = self.puts(calls)
        self.assertEqual((track, body["track"]), ("alpha", "alpha"))
        release = body["releases"][0]
        self.assertEqual(release["status"], "completed")
        [note] = release["releaseNotes"]
        self.assertEqual(note["language"], "en-US")
        self.assertEqual(len(note["text"]), ci.PLAY_NOTES_LIMIT)
        self.assertTrue(note["text"].startswith("Fixes\n"))
        self.assertTrue(calls[-1][1].endswith("/edits/e1:commit"))

    def test_upload_track_comes_from_play_track_and_production_is_refused_before_any_call(self):
        responses = {("POST", "/edits"): {"id": "e1"}, ("POST", "bundles?uploadType=media"): {"versionCode": 301036}}
        def upload(call):
            os.environ["PLAY_TRACK"] = "beta"
            return ci.upload_play(INFO, call=call)
        outcome, calls = self.run_play(upload, responses)
        self.assertEqual(self.puts(calls)[0][0], "beta")
        self.assertNotIn("releaseNotes", self.puts(calls)[0][1]["releases"][0])
        outcome, calls = self.run_play(lambda call: ci.upload_play(INFO, call=call, track="production"), responses)
        self.assertIsInstance(outcome, RuntimeError)
        self.assertEqual(calls, [])

    def test_draft_app_on_a_closed_track_still_falls_back_to_draft(self):
        draft = ci.PlayError(400, "Only releases with status draft may be created on draft app.")
        outcome, calls = self.run_play(lambda call: ci.upload_play(INFO, call=call, track="alpha"),
                                       {("POST", "/edits"): {"id": "e1"},
                                        ("POST", "bundles?uploadType=media"): {"versionCode": 301036},
                                        ("POST", ":commit"): [draft, {}]})
        self.assertEqual(outcome, "uploaded")
        self.assertEqual([(t, b["releases"][0]["status"]) for t, b in self.puts(calls)],
                         [("alpha", "completed"), ("alpha", "draft")])

    def test_rerun_checks_the_selected_track(self):
        used = ci.PlayError(403, "APK specifies a version code that has already been used.")
        outcome, calls = self.run_play(lambda call: ci.upload_play(INFO, call=call, track="beta"),
                                       {("POST", "/edits"): {"id": "e1"}, ("POST", "bundles?uploadType=media"): used,
                                        ("GET", "/tracks/beta"): {"releases": []}})
        self.assertIsInstance(outcome, RuntimeError)
        self.assertIn("promote.yml", str(outcome))
        self.assertIn(("GET", ci.PLAY_API + "/com.UBSEDS.GS26/edits/e1/tracks/beta", None), calls)
        self.assertEqual(calls[-1][0], "DELETE")

    def test_promotion_moves_the_version_code_up_a_testing_track(self):
        outcome, calls = self.run_play(lambda call: ci.promote_play(INFO, "internal", "alpha", call=call),
                                       {("POST", "/edits"): {"id": "e2"},
                                        ("GET", "/tracks/internal"): {"releases": [{"versionCodes": ["301036"]}]},
                                        ("GET", "/tracks/alpha"): {"releases": [{"versionCodes": ["301035"]}]}})
        self.assertEqual(outcome, "promoted")
        self.assertEqual(self.puts(calls), [("alpha", {"track": "alpha", "releases": [
            {"name": "Version 0.3.1 Build 36", "versionCodes": ["301036"], "status": "completed"}]})])
        self.assertTrue(calls[-1][1].endswith("/edits/e2:commit"))
        self.assertFalse([c for c in calls if "production" in c[1]])

    def test_promotion_no_ops_when_already_there_and_refuses_bad_moves(self):
        outcome, calls = self.run_play(lambda call: ci.promote_play(INFO, "alpha", "beta", call=call),
                                       {("POST", "/edits"): {"id": "e2"},
                                        ("GET", "/tracks/alpha"): {"releases": [{"versionCodes": ["301036"]}]},
                                        ("GET", "/tracks/beta"): {"releases": [{"versionCodes": ["301036"]}]}})
        self.assertEqual(outcome, "already-promoted")
        self.assertEqual(self.puts(calls), [])
        self.assertEqual(calls[-1][0], "DELETE")
        outcome, calls = self.run_play(lambda call: ci.promote_play(INFO, "internal", "beta", call=call),
                                       {("POST", "/edits"): {"id": "e2"},
                                        ("GET", "/tracks/internal"): {"releases": []}})
        self.assertRegex(str(outcome), "not on the internal track")
        self.assertEqual(calls[-1][0], "DELETE")
        for source, target, message in [("beta", "alpha", "not a promotion"), ("alpha", "alpha", "not a promotion"),
                                        ("internal", "production", "production is refused"),
                                        ("production", "beta", "production is refused")]:
            with self.subTest(source=source, target=target):
                outcome, calls = self.run_play(lambda call: ci.promote_play(INFO, source, target, call=call), {})
                self.assertRegex(str(outcome), message)
                self.assertEqual(calls, [])


APP = {"data": [{"id": "app1", "type": "apps", "attributes": {"bundleId": "com.UBSEDS.GS26"}}]}
GROUPS = {"data": [{"id": "g-int", "type": "betaGroups", "attributes": {"name": "SEDS Team", "isInternalGroup": True}},
                   {"id": "g-ext", "type": "betaGroups", "attributes": {"name": "Launch Crew", "isInternalGroup": False}}]}


def build(state, encryption=False):
    return {"data": [{"id": "b1", "type": "builds",
                      "attributes": {"version": "36", "processingState": state, "usesNonExemptEncryption": encryption}}]}


class TestFlightGroups(unittest.TestCase):
    def distribute(self, groups, responses, notes=None, timeout=3600, interval=60):
        calls, slept = [], []
        lookup = recorded(responses, calls)
        def call(method, path, body=None, query=None):
            ci.asc_allowed(method, path)        # every recorded call must be on the allowlist
            calls.append((method, path, body, query))
            return lookup(method, path)
        now = [0.0]
        def sleep(seconds):
            slept.append(seconds)
            now[0] += seconds
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            if notes is not None:
                (root / "docs/release-notes").mkdir(parents=True)
                (root / "docs/release-notes/v0.3.1b36.md").write_text(notes)
            with patch.object(ci, "ROOT", root), patch.dict(os.environ, {}, clear=True), \
                 contextlib.redirect_stdout(io.StringIO()) as stdout:
                try:
                    outcome = ci.testflight_distribute(INFO, groups, call, timeout=timeout, interval=interval,
                                                       sleep=sleep, clock=lambda: now[0])
                except RuntimeError as error:
                    outcome = error
        return outcome, calls, slept, stdout.getvalue()

    def test_no_groups_makes_no_calls(self):
        outcome, calls, _, _ = self.distribute([], {})
        self.assertEqual((outcome, calls), ("no-groups", []))

    def test_waits_for_processing_then_adds_internal_groups_without_review(self):
        responses = {("GET", "/v1/apps"): APP,
                     ("GET", "/v1/builds"): [{"data": []}, build("PROCESSING"), build("VALID")],
                     ("GET", "/v1/betaGroups"): GROUPS}
        outcome, calls, slept, stdout = self.distribute(["SEDS Team"], responses)
        self.assertEqual(outcome, "distributed")
        self.assertEqual(slept, [60, 60])
        builds = [c for c in calls if c[1] == "/v1/builds"]
        self.assertEqual(builds[0][3], {"filter[app]": "app1", "filter[version]": "36",
                                        "filter[preReleaseVersion.version]": "0.3.1",
                                        "filter[preReleaseVersion.platform]": "IOS", "limit": "10"})
        self.assertIn(("POST", "/v1/builds/b1/relationships/betaGroups",
                       {"data": [{"type": "betaGroups", "id": "g-int"}]}, None), calls)
        self.assertFalse([c for c in calls if "betaAppReviewSubmissions" in c[1] or "Localizations" in c[1]])
        self.assertNotIn("::warning", stdout)

    def test_external_group_sets_what_to_test_and_submits_for_beta_review(self):
        responses = {("GET", "/v1/apps"): APP, ("GET", "/v1/builds"): build("VALID"),
                     ("GET", "/v1/betaGroups"): GROUPS,
                     ("GET", "/v1/betaBuildLocalizations"): {"data": [
                         {"id": "loc1", "attributes": {"locale": "en-US", "whatsNew": None}}]},
                     ("GET", "/v1/betaAppReviewSubmissions"): {"data": []}}
        outcome, calls, slept, stdout = self.distribute(["SEDS Team", "Launch Crew"], responses,
                                                        notes="Try the new fill screen.\n")
        self.assertEqual(outcome, "distributed")
        self.assertEqual(slept, [])
        writes = [(m, p, b) for m, p, b, _ in calls if m != "GET"]
        self.assertEqual(writes, [
            ("PATCH", "/v1/betaBuildLocalizations/loc1",
             {"data": {"type": "betaBuildLocalizations", "id": "loc1",
                       "attributes": {"whatsNew": "Try the new fill screen."}}}),
            ("POST", "/v1/builds/b1/relationships/betaGroups",
             {"data": [{"type": "betaGroups", "id": "g-int"}, {"type": "betaGroups", "id": "g-ext"}]}),
            ("POST", "/v1/betaAppReviewSubmissions",
             {"data": {"type": "betaAppReviewSubmissions",
                       "relationships": {"build": {"data": {"type": "builds", "id": "b1"}}}}})])
        self.assertNotIn("::warning", stdout)

    def test_new_localization_existing_review_and_missing_notes_warn(self):
        responses = {("GET", "/v1/apps"): APP, ("GET", "/v1/builds"): build("VALID", encryption=None),
                     ("GET", "/v1/betaGroups"): GROUPS, ("GET", "/v1/betaBuildLocalizations"): {"data": []},
                     ("GET", "/v1/betaAppReviewSubmissions"): {"data": [
                         {"id": "r1", "attributes": {"betaReviewState": "WAITING_FOR_REVIEW"}}]}}
        outcome, calls, _, _ = self.distribute(["Launch Crew"], responses, notes="Notes")
        self.assertEqual(outcome, "distributed")
        self.assertIn(("POST", "/v1/betaBuildLocalizations",
                       {"data": {"type": "betaBuildLocalizations", "attributes": {"locale": "en-US", "whatsNew": "Notes"},
                                 "relationships": {"build": {"data": {"type": "builds", "id": "b1"}}}}}, None), calls)
        self.assertFalse([c for c in calls if c[:2] == ("POST", "/v1/betaAppReviewSubmissions")])
        outcome, calls, _, stdout = self.distribute(["Launch Crew"], responses)
        self.assertIn("::warning title=TestFlight What to Test::", stdout)
        self.assertIn("::warning title=TestFlight export compliance::", stdout)

    def test_processing_timeout_warns_and_does_not_fail(self):
        responses = {("GET", "/v1/apps"): APP, ("GET", "/v1/builds"): build("PROCESSING")}
        outcome, calls, slept, stdout = self.distribute(["Launch Crew"], responses, timeout=3600, interval=60)
        self.assertEqual(outcome, "processing-timeout")
        self.assertEqual(sum(slept), 3600)
        self.assertIn("::warning title=TestFlight processing::", stdout)
        self.assertIn("promote.yml", stdout)
        self.assertEqual({c[1] for c in calls}, {"/v1/apps", "/v1/builds"})

    def test_failed_processing_unknown_group_and_missing_app_fail(self):
        for responses, message in [
                ({("GET", "/v1/apps"): APP, ("GET", "/v1/builds"): build("INVALID")}, "INVALID"),
                ({("GET", "/v1/apps"): APP, ("GET", "/v1/builds"): build("VALID"),
                  ("GET", "/v1/betaGroups"): GROUPS}, "No TestFlight beta group named Nobody"),
                ({("GET", "/v1/apps"): {"data": []}}, "no app with bundle id")]:
            with self.subTest(message=message):
                outcome, calls, _, _ = self.distribute(["Nobody"], responses)
                self.assertRegex(str(outcome), message)
                self.assertFalse([c for c in calls if c[0] != "GET"])

    def test_allowlist_refuses_app_store_review_and_other_calls(self):
        for method, path in [("POST", "/v1/appStoreVersionSubmissions"), ("POST", "/v1/reviewSubmissions"),
                             ("PATCH", "/v1/appStoreVersions/x"), ("POST", "/v1/reviewSubmissionItems"),
                             ("DELETE", "/v1/builds/b1/relationships/betaGroups"), ("PATCH", "/v1/builds/b1"),
                             ("POST", "/v1/builds/b1/relationships/betaGroups/../../x")]:
            with self.subTest(method=method, path=path), self.assertRaisesRegex(RuntimeError, "Refusing"):
                ci.asc_allowed(method, path)
        with patch.object(ci, "asc_token", side_effect=AssertionError("no token for refused calls")), \
             self.assertRaisesRegex(RuntimeError, "Refusing"):
            ci.AppStoreConnect("ABC123XYZ9", "issuer")("POST", "/v1/appStoreVersionSubmissions", body={})

    def test_store_upload_without_groups_is_unchanged(self):
        with patch.object(ci, "release_version", return_value=INFO), \
             patch.object(ci, "upload_testflight", return_value="uploaded") as upload, \
             patch.object(ci, "testflight_distribute", side_effect=AssertionError("no groups, no ASC calls")), \
             patch.object(ci, "AppStoreConnect", side_effect=AssertionError("no groups, no ASC client")):
            for value in [None, "", "none"]:
                env = {} if value is None else {"TESTFLIGHT_GROUPS": value}
                with self.subTest(value=value), patch.dict(os.environ, env, clear=True):
                    self.assertEqual(ci.store_upload("ios", "v0.3.1b36"), "uploaded")
        self.assertEqual(upload.call_count, 3)

    def test_store_upload_with_groups_distributes_after_the_upload(self):
        events = []
        with patch.object(ci, "release_version", return_value=INFO), \
             patch.object(ci, "upload_testflight", side_effect=lambda info: events.append("upload") or "uploaded"), \
             patch.object(ci, "asc_credentials", return_value=("ABC123XYZ9", "issuer")), \
             patch.object(ci, "testflight_distribute",
                          side_effect=lambda info, groups, call: events.append(("groups", groups))), \
             patch.dict(os.environ, {"TESTFLIGHT_GROUPS": "Launch Crew"}, clear=True):
            ci.store_upload("ios", "v0.3.1b36")
        self.assertEqual(events, ["upload", ("groups", ["Launch Crew"])])
        # A bad group list fails before the (long) upload.
        with patch.object(ci, "release_version", return_value=INFO), \
             patch.object(ci, "upload_testflight", side_effect=AssertionError("must not upload")), \
             patch.dict(os.environ, {"TESTFLIGHT_GROUPS": "bad\nname"}, clear=True), self.assertRaises(RuntimeError):
            ci.store_upload("ios", "v0.3.1b36")


class AppStoreConnectToken(unittest.TestCase):
    def test_es256_token_verifies_and_the_key_is_removed(self):
        with tempfile.TemporaryDirectory() as temp, tempfile.TemporaryDirectory() as runner:
            key = Path(temp) / "key.p8"
            subprocess.run(["openssl", "genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256",
                            "-out", str(key)], check=True, capture_output=True)
            env = {"RUNNER_TEMP": runner, "APPLE_NOTARY_KEY_P8_B64": base64.b64encode(key.read_bytes()).decode()}
            with patch.dict(os.environ, env, clear=True), contextlib.redirect_stdout(io.StringIO()) as stdout:
                token = ci.asc_token("ABC123XYZ9", "issuer-uuid", now=1_800_000_000)
            self.assertIn(f"::add-mask::{token}", stdout.getvalue())
            self.assertEqual(list((Path(runner) / "seds-release-signing").iterdir()), [], "p8 removed after signing")
            header, claims, signature = token.split(".")
            pad = lambda part: base64.urlsafe_b64decode(part + "=" * (-len(part) % 4))
            self.assertEqual(json.loads(pad(header)), {"alg": "ES256", "kid": "ABC123XYZ9", "typ": "JWT"})
            self.assertEqual(json.loads(pad(claims)), {"iss": "issuer-uuid", "iat": 1_800_000_000,
                                                       "exp": 1_800_000_000 + 1200, "aud": "appstoreconnect-v1"})
            raw = pad(signature)
            self.assertEqual(len(raw), 64)
            def integer(value):
                body = value.lstrip(b"\0") or b"\0"
                if body[0] & 0x80:
                    body = b"\0" + body
                return b"\x02" + bytes([len(body)]) + body
            sequence = integer(raw[:32]) + integer(raw[32:])
            (Path(temp) / "sig.der").write_bytes(b"\x30" + bytes([len(sequence)]) + sequence)
            (Path(temp) / "signed").write_bytes(f"{header}.{claims}".encode())
            public = subprocess.run(["openssl", "pkey", "-in", str(key), "-pubout", "-out", f"{temp}/pub.pem"],
                                    capture_output=True)
            self.assertEqual(public.returncode, 0)
            verify = subprocess.run(["openssl", "dgst", "-sha256", "-verify", f"{temp}/pub.pem", "-signature",
                                     f"{temp}/sig.der", f"{temp}/signed"], capture_output=True, text=True)
            self.assertEqual(verify.returncode, 0, verify.stdout + verify.stderr)

    def test_der_conversion_pads_and_refuses_garbage(self):
        der = bytes([0x30, 0x08, 0x02, 0x02, 0x00, 0x80, 0x02, 0x02, 0x01, 0x02])
        self.assertEqual(ci.der_ecdsa_to_raw(der), (0x80).to_bytes(32, "big") + (0x0102).to_bytes(32, "big"))
        for bad in [b"", b"\x31\x00", b"\x30\x02\x05\x00"]:
            with self.subTest(bad=bad), self.assertRaisesRegex(RuntimeError, "malformed"):
                ci.der_ecdsa_to_raw(bad)


if __name__ == "__main__":
    unittest.main()

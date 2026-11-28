"""The Apple Watch app in the TestFlight IPA: the optional watch / widget App
Store profiles from secrets, the store build that embeds the watch app, and the
IPA verification of an embedded watch app. Nothing here runs Apple tools:
`security cms`, `codesign`, `ditto` and `vtool` are replaced by fixtures."""
import base64
import contextlib
import datetime
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("frontend_ci_watch_store", ROOT / "scripts/ci/frontend.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)
targets_spec = importlib.util.spec_from_file_location("watch_store_targets", ROOT / "scripts/build_targets.py")
targets = importlib.util.module_from_spec(targets_spec)
targets_spec.loader.exec_module(targets)
deploy_spec = importlib.util.spec_from_file_location("watch_store_deploy", ROOT / "scripts/watch_deploy.py")
deploy = importlib.util.module_from_spec(deploy_spec)
deploy_spec.loader.exec_module(deploy)

TEAM = "TEAM123456"
NOW = datetime.datetime(2026, 11, 27)
CERT = b"apple-distribution-certificate"
CERT_SHA1 = hashlib.sha1(CERT).hexdigest().upper()
AUTHORITY = "Apple Distribution: UB SEDS (TEAM123456)"
PHONE, WATCH, WIDGET = "com.UBSEDS.GS26", "com.UBSEDS.GS26.watchkitapp", "com.UBSEDS.GS26.watchkitapp.widgets"


def profile(bundle_id, team=TEAM, **changes):
    info = {"TeamIdentifier": [team], "Name": f"{bundle_id} App Store", "ExpirationDate": datetime.datetime(2027, 11, 1),
            "DeveloperCertificates": [CERT],
            "Entitlements": {"application-identifier": f"{team}.{bundle_id}",
                             "com.apple.developer.team-identifier": team, "get-task-allow": False,
                             "beta-reports-active": True}}
    info.update(changes)
    return info


def b64(text):
    return base64.b64encode(text.encode()).decode()


class Identifiers(unittest.TestCase):
    def test_watch_ids_match_the_build_scripts(self):
        self.assertEqual(ci.IOS_WATCH_BUNDLE_ID, ci.IOS_BUNDLE_ID + deploy.WATCH_BUNDLE_SUFFIX)
        self.assertEqual(ci.IOS_WATCH_WIDGETS_BUNDLE_ID, ci.IOS_WATCH_BUNDLE_ID + targets.WATCH_WIDGET_BUNDLE_SUFFIX)
        self.assertEqual(ci.WATCH_APP_NAME, targets.WATCH_APP_NAME)
        self.assertEqual(ci.WATCH_WIDGET_APPEX, targets.WATCH_WIDGET_APPEX)
        self.assertEqual(ci.IOS_WATCH_PROFILE_ENV, targets.WATCH_DISTRIBUTION_PROFILE_ENV)
        # build.py reads exactly these names for distribution signing.
        build_py = (ROOT / "build.py").read_text()
        for name in [ci.IOS_WATCH_PROFILE_ENV, ci.IOS_WATCH_WIDGETS_PROFILE_ENV]:
            self.assertIn(f'"{name}"', build_py)


class WatchProfilesFromSecrets(unittest.TestCase):
    """prepare_watch_release_signing: decode, validate, export."""

    def run_prepare(self, secrets, profiles):
        """secrets: {secret: profile key}; profiles: {profile key: info}. Returns
        (outcome, GITHUB_ENV text, GITHUB_OUTPUT text, summary/stdout, files)."""
        with tempfile.TemporaryDirectory() as runner:
            env_file, out_file = Path(runner) / "env", Path(runner) / "out"
            values = {"RUNNER_TEMP": runner, "GITHUB_ENV": str(env_file), "GITHUB_OUTPUT": str(out_file),
                      **{secret: b64(key) for secret, key in secrets.items()}}
            def info(path):
                return profiles[Path(path).read_text()]
            with patch.dict(os.environ, values, clear=True), patch.object(ci, "mobileprovision_info", side_effect=info), \
                 contextlib.redirect_stdout(io.StringIO()) as stdout:
                directory = ci.release_signing_dir(create=True)
                try:
                    outcome = ci.prepare_watch_release_signing(directory, TEAM, CERT_SHA1, now=NOW)
                except RuntimeError as error:
                    outcome = error
                files = sorted(p.name for p in directory.iterdir())
                modes = {p.name: p.stat().st_mode & 0o777 for p in directory.iterdir()}
            read = lambda p: p.read_text() if p.exists() else ""
            return outcome, read(env_file), read(out_file), stdout.getvalue(), files, modes, runner

    def test_both_profiles_are_validated_and_exported_for_build_py(self):
        outcome, env, out, stdout, files, modes, runner = self.run_prepare(
            {ci.IOS_WATCH_PROFILE_SECRET: "watch", ci.IOS_WATCH_WIDGETS_PROFILE_SECRET: "widget"},
            {"watch": profile(WATCH), "widget": profile(WIDGET)})
        self.assertIsInstance(outcome, list)
        self.assertEqual(files, ["watch-appstore.mobileprovision", "watch-widgets-appstore.mobileprovision"])
        self.assertEqual(set(modes.values()), {0o600})
        directory = Path(runner).resolve() / "seds-release-signing"
        self.assertEqual(env, f"IOS_WATCH_DISTRIBUTION_MOBILEPROVISION={directory}/watch-appstore.mobileprovision\n"
                              f"IOS_WATCH_WIDGETS_DISTRIBUTION_MOBILEPROVISION={directory}/"
                              "watch-widgets-appstore.mobileprovision\n")
        self.assertEqual(out, "watch=true\nwatch_widget=true\n")
        self.assertIn("**embedded**", outcome[0])
        self.assertIn("with its push-to-talk widget", outcome[0])
        # Expiry reported like the iPhone profile.
        self.assertIn("Apple Watch app App Store profile: valid until 2027-11-01", stdout)
        self.assertIn("Apple Watch widget App Store profile: valid until 2027-11-01", stdout)

    def test_missing_watch_secret_keeps_the_iphone_only_release(self):
        outcome, env, out, stdout, files, _modes, _ = self.run_prepare({}, {})
        self.assertEqual(env, "")
        self.assertEqual(files, [])
        self.assertEqual(out, "watch=false\nwatch_widget=false\n")
        self.assertIn("**not embedded**", outcome[0])
        self.assertIn(ci.IOS_WATCH_PROFILE_SECRET, outcome[0])
        self.assertNotIn("::error", stdout)

    def test_widget_secret_alone_is_ignored_with_a_warning(self):
        outcome, env, out, stdout, files, _modes, _ = self.run_prepare(
            {ci.IOS_WATCH_WIDGETS_PROFILE_SECRET: "widget"}, {"widget": profile(WIDGET)})
        self.assertEqual((env, files, out), ("", [], "watch=false\nwatch_widget=false\n"))
        self.assertIn("ignored", outcome[1])
        self.assertIn("::warning title=Apple Watch app not embedded::", stdout)

    def test_missing_widget_secret_signs_the_watch_app_without_the_widget(self):
        outcome, env, out, stdout, files, _modes, _ = self.run_prepare(
            {ci.IOS_WATCH_PROFILE_SECRET: "watch"}, {"watch": profile(WATCH)})
        self.assertEqual(files, ["watch-appstore.mobileprovision"])
        self.assertIn("IOS_WATCH_DISTRIBUTION_MOBILEPROVISION=", env)
        self.assertNotIn("WIDGETS", env)
        self.assertEqual(out, "watch=true\nwatch_widget=false\n")
        self.assertIn("**without** its push-to-talk widget", outcome[0])
        self.assertIn("::notice title=Watch widget not included::", stdout)

    def test_bad_profiles_fail_the_job_and_export_nothing(self):
        cases = [
            ("wrong bundle id", profile(PHONE), profile(WIDGET), "IOS_WATCH_DISTRIBUTION_MOBILEPROVISION_B64 is not a profile for com.UBSEDS.GS26.watchkitapp"),
            ("widget id for watch", profile(WIDGET), profile(WIDGET), "not a profile for com.UBSEDS.GS26.watchkitapp "),
            ("watch id for widget", profile(WATCH), profile(WATCH), "WIDGETS_DISTRIBUTION_MOBILEPROVISION_B64 is not a profile"),
            ("wrong team", profile(WATCH, team="OTHERTEAM1"), profile(WIDGET), "belongs to team OTHERTEAM1"),
            ("widget wrong team", profile(WATCH), profile(WIDGET, team="OTHERTEAM1"), "WIDGETS.*belongs to team OTHERTEAM1"),
            ("missing certificate", profile(WATCH, DeveloperCertificates=[b"other"]), profile(WIDGET),
             "does not include the IOS_DISTRIBUTION_P12_B64 certificate"),
            ("widget missing certificate", profile(WATCH), profile(WIDGET, DeveloperCertificates=[]),
             "WIDGETS.*does not include"),
            ("expired", profile(WATCH, ExpirationDate=datetime.datetime(2026, 11, 1)), profile(WIDGET), "has expired"),
            ("widget expired", profile(WATCH), profile(WIDGET, ExpirationDate=datetime.datetime(2026, 1, 1)),
             "WIDGETS.*has expired"),
            ("ad hoc", profile(WATCH, ProvisionedDevices=["00008006-1"]), profile(WIDGET), "development, ad hoc"),
            ("development", profile(WATCH, Entitlements={"application-identifier": f"{TEAM}.{WATCH}",
                                                         "get-task-allow": True}), profile(WIDGET), "get-task-allow"),
        ]
        for label, watch, widget, message in cases:
            with self.subTest(label):
                outcome, env, out, _stdout, _files, _modes, _ = self.run_prepare(
                    {ci.IOS_WATCH_PROFILE_SECRET: "watch", ci.IOS_WATCH_WIDGETS_PROFILE_SECRET: "widget"},
                    {"watch": watch, "widget": widget})
                self.assertIsInstance(outcome, RuntimeError)
                self.assertRegex(str(outcome), message)
                self.assertEqual((env, out), ("", ""))

    def test_invalid_base64_fails(self):
        with tempfile.TemporaryDirectory() as runner, \
             patch.dict(os.environ, {"RUNNER_TEMP": runner, ci.IOS_WATCH_PROFILE_SECRET: "not base64!"}, clear=True), \
             contextlib.redirect_stdout(io.StringIO()):
            directory = ci.release_signing_dir(create=True)
            with self.assertRaisesRegex(RuntimeError, "not valid base64"):
                ci.prepare_watch_release_signing(directory, TEAM, CERT_SHA1, now=NOW)


class IosSigningStep(unittest.TestCase):
    """release-ios-signing end to end with the keychain calls replaced."""

    def run_step(self, extra):
        with tempfile.TemporaryDirectory() as runner:
            env_file, summary = Path(runner) / "env", Path(runner) / "summary"
            values = {"RUNNER_TEMP": runner, "GITHUB_ENV": str(env_file), "GITHUB_STEP_SUMMARY": str(summary),
                      "IOS_DISTRIBUTION_P12_B64": b64("p12"), "IOS_DISTRIBUTION_P12_PASSWORD": "pw",
                      "IOS_APPSTORE_PROFILE_B64": b64("phone"), **extra}
            profiles = {"phone": profile(PHONE), "watch": profile(WATCH), "widget": profile(WIDGET)}
            with patch.dict(os.environ, values, clear=True), \
                 patch.object(ci, "mobileprovision_info", side_effect=lambda p: profiles[Path(p).read_text()]), \
                 patch.object(ci, "create_release_keychain", side_effect=lambda d, *a: d / ci.KEYCHAIN_NAME), \
                 patch.object(ci, "codesigning_identity_hashes", return_value=[(CERT_SHA1, AUTHORITY)]), \
                 patch.object(ci, "keychain_cert_not_after", return_value=datetime.datetime(2027, 6, 1)), \
                 patch.object(ci, "utc_now", return_value=NOW), contextlib.redirect_stdout(io.StringIO()):
                ci.prepare_ios_release_signing()
            return env_file.read_text(), summary.read_text()

    def test_without_watch_secrets_the_step_is_unchanged_apart_from_the_summary(self):
        env, summary = self.run_step({})
        self.assertEqual([line.split("=")[0] for line in env.splitlines()], ["IOS_DISTRIBUTION_MOBILEPROVISION"])
        self.assertIn("App Store signing: `Apple Distribution: UB SEDS (TEAM123456)`", summary)
        self.assertIn("Apple Watch app: **not embedded**", summary)

    def test_with_watch_secrets_both_profiles_are_exported(self):
        env, summary = self.run_step({ci.IOS_WATCH_PROFILE_SECRET: b64("watch"),
                                      ci.IOS_WATCH_WIDGETS_PROFILE_SECRET: b64("widget")})
        self.assertEqual([line.split("=")[0] for line in env.splitlines()],
                         ["IOS_DISTRIBUTION_MOBILEPROVISION", "IOS_WATCH_DISTRIBUTION_MOBILEPROVISION",
                          "IOS_WATCH_WIDGETS_DISTRIBUTION_MOBILEPROVISION"])
        self.assertIn("Apple Watch app: **embedded**", summary)

    def test_cleanup_removes_the_watch_profiles_with_the_rest(self):
        with tempfile.TemporaryDirectory() as runner, tempfile.TemporaryDirectory() as checkout, \
             patch.object(ci, "ROOT", Path(checkout)), patch.dict(os.environ, {"RUNNER_TEMP": runner}, clear=True), \
             contextlib.redirect_stdout(io.StringIO()):
            directory = ci.release_signing_dir(create=True)
            for name in ["appstore.mobileprovision", "watch-appstore.mobileprovision",
                         "watch-widgets-appstore.mobileprovision"]:
                (directory / name).write_bytes(b"x")
            ci.release_cleanup()
            self.assertFalse(directory.exists())


class WatchStoreBuild(unittest.TestCase):
    def test_store_command_embeds_the_watch_app_only_with_its_profile(self):
        self.assertEqual(ci.store_command("ios")[1:], ["build.py", "--target", "ios", "--release", "--final"])
        self.assertEqual(ci.store_command("ios", watch=True)[1:],
                         ["build.py", "--target", "watchos", "--release", "--final"])

    def test_release_env_passes_only_job_local_watch_profiles(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp).resolve() / "seds-release-signing"
            values = {"RUNNER_TEMP": temp, "IOS_DISTRIBUTION_MOBILEPROVISION": str(directory / "appstore.mobileprovision"),
                      ci.IOS_WATCH_PROFILE_ENV: str(directory / "watch-appstore.mobileprovision"),
                      ci.IOS_WATCH_WIDGETS_PROFILE_ENV: str(directory / "watch-widgets-appstore.mobileprovision"),
                      ci.IOS_WATCH_PROFILE_SECRET: "fixture", ci.IOS_WATCH_WIDGETS_PROFILE_SECRET: "fixture"}
            with patch.dict(os.environ, values, clear=True):
                env = ci.release_env("ios")
            self.assertEqual(env[ci.IOS_WATCH_PROFILE_ENV], values[ci.IOS_WATCH_PROFILE_ENV])
            self.assertEqual(env[ci.IOS_WATCH_WIDGETS_PROFILE_ENV], values[ci.IOS_WATCH_WIDGETS_PROFILE_ENV])
            self.assertNotIn(ci.IOS_WATCH_PROFILE_SECRET, env)
            self.assertNotIn(ci.IOS_WATCH_WIDGETS_PROFILE_SECRET, env)
            for key in [ci.IOS_WATCH_PROFILE_ENV, ci.IOS_WATCH_WIDGETS_PROFILE_ENV]:
                with self.subTest(key=key), patch.dict(os.environ, {**values, key: "/elsewhere/p.mobileprovision"},
                                                       clear=True), self.assertRaisesRegex(RuntimeError, "job-local"):
                    ci.release_env("ios")
            widget_only = {k: v for k, v in values.items() if k != ci.IOS_WATCH_PROFILE_ENV}
            with patch.dict(os.environ, widget_only, clear=True):
                self.assertNotIn(ci.IOS_WATCH_WIDGETS_PROFILE_ENV, ci.release_env("ios"))

    def test_build_only_env_drops_watch_profiles_and_secrets(self):
        names = [ci.IOS_WATCH_PROFILE_ENV, ci.IOS_WATCH_WIDGETS_PROFILE_ENV, ci.IOS_WATCH_PROFILE_SECRET,
                 ci.IOS_WATCH_WIDGETS_PROFILE_SECRET, "IOS_WATCH_DEVELOPMENT_MOBILEPROVISION"]
        with patch.dict(os.environ, {name: "fixture" for name in names}):
            env = ci.ci_env()
        self.assertFalse(set(names) & set(env))

    def run_store_build(self, watch, which="/usr/local/bin/xcodegen"):
        with tempfile.TemporaryDirectory() as runner, tempfile.TemporaryDirectory() as temp:
            directory = Path(runner).resolve() / "seds-release-signing"
            values = {"RUNNER_TEMP": runner, "IOS_DISTRIBUTION_MOBILEPROVISION": str(directory / "appstore.mobileprovision")}
            if watch:
                values[ci.IOS_WATCH_PROFILE_ENV] = str(directory / "watch-appstore.mobileprovision")
            seen = []
            def checked(argv, env, label, **kwargs):
                seen.append((argv, env))
                output = Path(temp) / ci.STORE_OUTPUTS["ios"]
                output.parent.mkdir(parents=True)
                output.write_bytes(b"ipa")
            with patch.object(ci, "ROOT", Path(temp)), patch.dict(os.environ, values, clear=True), \
                 patch.object(ci, "validate_host"), patch.object(ci, "checked", side_effect=checked), \
                 patch.object(ci.shutil, "which", side_effect=lambda tool, path=None: None if tool == "xcodegen" and not which else f"/bin/{tool}"):
                ci.store_build("ios")
            return seen

    def test_store_build_selects_the_watchos_target_with_the_watch_profile(self):
        (argv, env), = self.run_store_build(watch=False)
        self.assertEqual(argv[1:4], ["build.py", "--target", "ios"])
        self.assertNotIn(ci.IOS_WATCH_PROFILE_ENV, env)
        (argv, env), = self.run_store_build(watch=True)
        self.assertEqual(argv[1:4], ["build.py", "--target", "watchos"])
        self.assertTrue(env[ci.IOS_WATCH_PROFILE_ENV].endswith("watch-appstore.mobileprovision"))
        with self.assertRaisesRegex(RuntimeError, "xcodegen"):
            self.run_store_build(watch=True, which=None)


def write_plist(bundle, values, binary=False):
    bundle.mkdir(parents=True, exist_ok=True)
    (bundle / "Info.plist").write_bytes(plistlib.dumps(values, fmt=plistlib.FMT_BINARY if binary else plistlib.FMT_XML))


def fake_payload(root, *, watch=True, widget=True, watch_info=None, widget_info=None):
    app = root / "Payload" / "GS26.app"
    write_plist(app, {"CFBundleIdentifier": PHONE, "CFBundleShortVersionString": "0.4.0", "CFBundleVersion": "40",
                      "CFBundleExecutable": "GS26", "MinimumOSVersion": "15.0"})
    (app / "GS26").write_bytes(b"binary")
    (app / "embedded.mobileprovision").write_text("phone")
    if watch:
        bundle = app / "Watch" / "GS26Watch.app"
        write_plist(bundle, {"CFBundleIdentifier": WATCH, "WKCompanionAppBundleIdentifier": PHONE,
                             "CFBundleShortVersionString": "0.4.0", "CFBundleVersion": "40", **(watch_info or {})},
                    binary=True)
        (bundle / "embedded.mobileprovision").write_text("watch")
        if widget:
            appex = bundle / "PlugIns" / "GS26WatchWidgets.appex"
            write_plist(appex, {"CFBundleIdentifier": WIDGET, "CFBundleShortVersionString": "0.4.0",
                                "CFBundleVersion": "40", **(widget_info or {})})
            (appex / "embedded.mobileprovision").write_text("widget")
    return app


class FakeApple:
    """subprocess.run stand-in for ditto, codesign and vtool on a fake IPA."""

    def __init__(self, payload_root, authority=None, teams=None, entitlements=None):
        self.payload_root = payload_root
        self.authority = authority or {}
        self.teams = teams or {}
        self.entitlements = entitlements or {}
        self.calls = []

    @staticmethod
    def kind(path):
        name = Path(path).name
        return {"GS26.app": "phone", "GS26Watch.app": "watch", "GS26WatchWidgets.appex": "widget"}[name]

    def default_entitlements(self, kind):
        bundle = {"phone": PHONE, "watch": WATCH, "widget": WIDGET}[kind]
        return {"application-identifier": f"{TEAM}.{bundle}", "com.apple.developer.team-identifier": TEAM,
                "get-task-allow": False, "beta-reports-active": True}

    def __call__(self, argv, **kwargs):
        self.calls.append(argv)
        text = kwargs.get("text")
        def done(out="", err="", code=0):
            if not text:
                out, err = out.encode() if isinstance(out, str) else out, err.encode()
            return subprocess.CompletedProcess(argv, code, out, err)
        if argv[0] == "ditto":
            shutil.copytree(self.payload_root / "Payload", Path(argv[-1]) / "Payload", symlinks=True)
            return done()
        if argv[:2] == ["xcrun", "vtool"]:
            return done("Load command 9\n      cmd LC_BUILD_VERSION\n platform IOS\n    minos 15.0\n      sdk 27.0\n")
        if argv[:2] == ["codesign", "--verify"]:
            return done()
        if argv[:2] == ["codesign", "-dvv"]:
            kind = self.kind(argv[-1])
            authority = self.authority.get(kind, AUTHORITY)
            team = self.teams.get(kind, TEAM)
            return done("", f"Executable={argv[-1]}\nAuthority={authority}\nAuthority=Apple Worldwide Developer "
                            f"Relations Certification Authority\nTeamIdentifier={team}\n")
        if argv[:4] == ["codesign", "-d", "--entitlements", "-"]:
            kind = self.kind(argv[-1])
            values = self.entitlements.get(kind, self.default_entitlements(kind))
            return done(plistlib.dumps(values))
        raise AssertionError(f"unexpected command {argv}")


INFO = {"tag": "v0.4.0b40", "version": "0.4.0", "build": "40"}


class StoreVerifyWatch(unittest.TestCase):
    def verify(self, *, expect_watch=True, expect_widget=True, profiles=None, apple=None, **payload):
        profiles = {"phone": profile(PHONE), "watch": profile(WATCH), "widget": profile(WIDGET), **(profiles or {})}
        with tempfile.TemporaryDirectory() as temp, tempfile.TemporaryDirectory() as source, \
             tempfile.NamedTemporaryFile() as summary:
            fake_payload(Path(source), **payload)
            fake = FakeApple(Path(source), **(apple or {}))
            values = {"GITHUB_STEP_SUMMARY": summary.name}
            if expect_watch:
                values[ci.IOS_WATCH_PROFILE_ENV] = "/runner/seds-release-signing/watch-appstore.mobileprovision"
            if expect_widget:
                values[ci.IOS_WATCH_WIDGETS_PROFILE_ENV] = "/runner/seds-release-signing/watch-widgets.mobileprovision"
            with patch.object(ci, "ROOT", Path(temp)), patch.dict(os.environ, values, clear=True), \
                 patch.object(ci.subprocess, "run", side_effect=fake), \
                 patch.object(ci, "mobileprovision_info", side_effect=lambda p: profiles[Path(p).read_text()]), \
                 patch.object(ci, "utc_now", return_value=NOW), contextlib.redirect_stdout(io.StringIO()):
                try:
                    ci.verify_store_ios(INFO)
                    outcome = Path(summary.name).read_text()
                except RuntimeError as error:
                    outcome = error
            return outcome, fake.calls

    def test_embedded_watch_app_and_widget_pass(self):
        summary, calls = self.verify()
        self.assertIsInstance(summary, str, summary)
        self.assertIn("Apple Watch app: **embedded** at `Watch/GS26Watch.app`", summary)
        self.assertIn(f"with its widget `{WIDGET}`", summary)
        checked = {Path(c[-1]).name for c in calls if c[:2] == ["codesign", "-dvv"]}
        self.assertEqual(checked, {"GS26.app", "GS26Watch.app", "GS26WatchWidgets.appex"})

    def test_watch_app_without_widget_passes_when_no_widget_profile(self):
        summary, _ = self.verify(expect_widget=False, widget=False)
        self.assertIsInstance(summary, str, summary)
        self.assertIn("without its push-to-talk widget", summary)

    def test_iphone_only_ipa_without_watch_profile_is_reported_not_failed(self):
        summary, calls = self.verify(expect_watch=False, expect_widget=False, watch=False)
        self.assertIsInstance(summary, str, summary)
        self.assertIn("Apple Watch app: **not embedded**", summary)
        self.assertFalse([c for c in calls if "GS26Watch.app" in " ".join(c)])

    def test_inconsistent_embedded_watch_apps_fail(self):
        other_team = {"application-identifier": f"OTHERTEAM1.{WATCH}", "com.apple.developer.team-identifier": "OTHERTEAM1"}
        cases = [
            ("watch app missing", dict(watch=False), "Expected Watch/GS26Watch.app"),
            ("unexpected watch app", dict(expect_watch=False, expect_widget=False), "no watch App Store profile"),
            ("wrong bundle id", dict(watch_info={"CFBundleIdentifier": "com.UBSEDS.GS26.other"}), "embedded watch app is"),
            ("wrong companion", dict(watch_info={"WKCompanionAppBundleIdentifier": "com.example"}),
             "WKCompanionAppBundleIdentifier"),
            ("watch version", dict(watch_info={"CFBundleShortVersionString": "1.0"}), "watch app has version/build"),
            ("watch build", dict(watch_info={"CFBundleVersion": "1"}), "rejects the IPA"),
            ("widget build", dict(widget_info={"CFBundleVersion": "1"}), "watch widget has version/build"),
            ("widget id", dict(widget_info={"CFBundleIdentifier": WATCH}), "embedded watch widget is"),
            ("widget missing", dict(widget=False), "Expected the watch widget"),
            ("unexpected widget", dict(expect_widget=False), "no widget App Store profile"),
            ("watch profile for another app", dict(profiles={"watch": profile(PHONE)}),
             "IOS_WATCH_DISTRIBUTION_MOBILEPROVISION_B64 is not a profile"),
            ("watch development profile", dict(profiles={"watch": profile(WATCH, ProvisionedDevices=["x"])}),
             "development, ad hoc"),
            ("watch profile of another team", dict(profiles={"watch": profile(WATCH, team="OTHERTEAM1")}),
             "not for team TEAM123456"),
            ("widget profile expired", dict(profiles={"widget": profile(WIDGET, ExpirationDate=datetime.datetime(2026, 1, 1))}),
             "has expired"),
            ("different signer", dict(apple={"authority": {"watch": "Apple Distribution: Someone Else (XYZ)"}}),
             "watch app is not signed by Apple Distribution: UB SEDS"),
            ("development signer", dict(apple={"authority": {"widget": "Apple Development: UB SEDS (TEAM123456)"}}),
             "watch widget is not signed by"),
            ("signed for another team", dict(apple={"teams": {"watch": "OTHERTEAM1"}}), "signed for team 'OTHERTEAM1'"),
            ("entitlements of another team", dict(apple={"entitlements": {"watch": other_team}}),
             "application-identifier 'OTHERTEAM1"),
            ("get-task-allow", dict(apple={"entitlements": {"widget": {
                "application-identifier": f"{TEAM}.{WIDGET}", "com.apple.developer.team-identifier": TEAM,
                "get-task-allow": True}}}), "get-task-allow"),
            ("entitlement not in profile", dict(apple={"entitlements": {"watch": {
                "application-identifier": f"{TEAM}.{WATCH}", "com.apple.developer.team-identifier": TEAM,
                "aps-environment": "production"}}}), "does not grant: aps-environment"),
        ]
        for label, kwargs, message in cases:
            with self.subTest(label):
                outcome, _ = self.verify(**kwargs)
                self.assertIsInstance(outcome, RuntimeError, label)
                self.assertRegex(str(outcome), message)


class WatchVersionStamp(unittest.TestCase):
    def test_embedding_gives_the_watch_app_and_widget_the_phone_version(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            phone = root / "dist" / "GS26.app"
            write_plist(phone, {"CFBundleIdentifier": PHONE, "CFBundleShortVersionString": "0.4.0",
                                "CFBundleVersion": "40"})
            product = root / "build" / "GS26Watch.app"
            write_plist(product, {"CFBundleIdentifier": WATCH, "WKCompanionAppBundleIdentifier": PHONE,
                                  "CFBundleShortVersionString": "1.0", "CFBundleVersion": "1"}, binary=True)
            write_plist(product / "PlugIns" / "GS26WatchWidgets.appex",
                        {"CFBundleIdentifier": WIDGET, "CFBundleShortVersionString": "1.0", "CFBundleVersion": "1"})
            embedded = targets.embed_watch_app(product, phone)
            watch_raw = (embedded / "Info.plist").read_bytes()
            self.assertTrue(watch_raw.startswith(b"bplist"), "binary plists stay binary")
            for bundle in [embedded, embedded / "PlugIns" / "GS26WatchWidgets.appex"]:
                info = plistlib.loads((bundle / "Info.plist").read_bytes())
                self.assertEqual((info["CFBundleShortVersionString"], info["CFBundleVersion"]), ("0.4.0", "40"))
            # The Xcode product itself is left alone.
            self.assertEqual(plistlib.loads((product / "Info.plist").read_bytes())["CFBundleVersion"], "1")


if __name__ == "__main__":
    unittest.main()

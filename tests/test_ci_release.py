"""Release workflow contracts: production signing happens only in the
tag-triggered release workflow, only from secrets, only in the signing jobs,
and the signing material is cleaned up and never published."""
import base64
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import yaml

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("frontend_ci_release", ROOT / "scripts/ci/frontend.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)

ANDROID_SECRETS = {"ANDROID_UPLOAD_KEYSTORE_B64", "ANDROID_KEYSTORE_PASSWORD", "ANDROID_KEY_PASSWORD", "ANDROID_KEY_ALIAS"}
MACOS_SECRETS = {"MACOS_DEVELOPER_ID_P12_B64", "MACOS_DEVELOPER_ID_P12_PASSWORD", "MACOS_SIGNING_IDENTITY",
                 "APPLE_NOTARY_KEY_P8_B64", "APPLE_NOTARY_KEY_ID", "APPLE_NOTARY_ISSUER_ID",
                 "APPLE_ID", "APPLE_APP_SPECIFIC_PASSWORD", "APPLE_TEAM_ID"}
TESTFLIGHT_SECRETS = {"IOS_DISTRIBUTION_P12_B64", "IOS_DISTRIBUTION_P12_PASSWORD", "IOS_APPSTORE_PROFILE_B64",
                      "APPLE_NOTARY_KEY_P8_B64", "APPLE_NOTARY_KEY_ID", "APPLE_NOTARY_ISSUER_ID"}
PLAY_SECRETS = ANDROID_SECRETS | {"PLAY_SERVICE_ACCOUNT_JSON_B64"}
SIGNING_JOBS = {"android": ANDROID_SECRETS, "macos": MACOS_SECRETS, "testflight": TESTFLIGHT_SECRETS,
                "play": PLAY_SECRETS}
STORE_JOBS = ["testflight", "play"]
SECRET = re.compile(r"secrets\.([A-Za-z0-9_]+)")


def secrets_in(node):
    return set(SECRET.findall(json.dumps(node)))


class ReleaseWorkflow(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.raw = (ROOT / ".github/workflows/release.yml").read_text()
        cls.workflow = yaml.safe_load(cls.raw)
        cls.jobs = cls.workflow["jobs"]

    def step(self, job, name):
        return next(s for s in self.jobs[job]["steps"] if s.get("name") == name)

    def test_triggered_only_by_version_tags_or_dispatch_with_tag(self):
        on = self.workflow["on"]
        self.assertEqual(set(on), {"push", "workflow_dispatch"})
        self.assertEqual(on["push"], {"tags": ["v*.*.*b*"]})
        self.assertTrue(on["workflow_dispatch"]["inputs"]["tag"]["required"])
        self.assertNotIn("pull_request", self.raw)
        self.assertNotIn("pull_request_target", self.raw)

    def test_tag_is_checked_against_cargo_and_dioxus_before_any_build(self):
        step = self.step("version", "Tag matches Cargo.toml version and Dioxus.toml build")
        self.assertIn("release-version", step["run"])
        for job in ["unsigned", "android", "macos", *STORE_JOBS]:
            self.assertEqual(self.jobs[job]["needs"], "version")
            checkout = self.jobs[job]["steps"][0]
            self.assertEqual(checkout["with"]["ref"], "${{ needs.version.outputs.ref }}")

    def test_secrets_only_in_the_signing_steps_of_the_signing_jobs(self):
        self.assertEqual(self.workflow["permissions"], {"contents": "read"})
        for name, job in self.jobs.items():
            allowed = SIGNING_JOBS.get(name, set())
            self.assertLessEqual(secrets_in(job), allowed, name)
            for step in job["steps"]:
                used = secrets_in(step)
                if used:
                    self.assertTrue(step.get("run", "").startswith("python scripts/ci/frontend.py release-"), step)
                    self.assertEqual(secrets_in(step.get("env", {})), used, "secrets only via step env")
        for name, expected in SIGNING_JOBS.items():
            self.assertEqual(secrets_in(self.jobs[name]), expected, name)
        self.assertEqual(secrets_in(self.jobs["version"]) | secrets_in(self.jobs["unsigned"]), set())
        self.assertNotIn("secrets", json.dumps(self.workflow.get("env", {})))

    def test_only_publish_can_write_and_it_holds_no_secrets(self):
        for name, job in self.jobs.items():
            if name == "publish":
                self.assertEqual(job["permissions"], {"contents": "write"})
            else:
                self.assertNotIn("permissions", job)
        self.assertEqual(secrets_in(self.jobs["publish"]), set())

    def test_signing_jobs_clean_up_last_and_always(self):
        for job in SIGNING_JOBS:
            last = self.jobs[job]["steps"][-1]
            self.assertEqual(last["if"], "always()")
            self.assertEqual(last["run"], "python scripts/ci/frontend.py release-cleanup")

    def test_signing_jobs_verify_before_staging_and_upload_only_staged_assets(self):
        for job in ["android", "macos", "unsigned"]:
            runs = [s.get("run") for s in self.jobs[job]["steps"]]
            build = runs.index("python scripts/ci/frontend.py release-build")
            stage = runs.index("python scripts/ci/frontend.py release-stage")
            self.assertLess(build, stage)
            if job != "unsigned":
                self.assertLess(build, runs.index("python scripts/ci/frontend.py release-verify"))
                self.assertLess(runs.index("python scripts/ci/frontend.py release-verify"), stage)
            uploads = [s for s in self.jobs[job]["steps"] if s.get("uses") == "actions/upload-artifact@v4"]
            assets = [u for u in uploads if u["with"]["name"] == "release-${{ matrix.target }}"]
            self.assertEqual(len(assets), 1)
            self.assertEqual(assets[0]["with"]["path"], "release-assets/${{ matrix.target }}/")
            for upload in uploads:
                self.assertNotIn("RUNNER_TEMP", json.dumps(upload))
                if upload is not assets[0]:
                    # Build logs only for the unsigned jobs, only on failure.
                    self.assertEqual(job, "unsigned")
                    self.assertEqual(upload["if"], "failure()")

    def test_release_targets_and_runners(self):
        unsigned = [(r["target"], r["runner"]) for r in self.jobs["unsigned"]["strategy"]["matrix"]["include"]]
        self.assertEqual(unsigned, [("linux", "ubuntu-24.04"), ("linux-arm64", "ubuntu-24.04-arm"), ("windows", "windows-2022")])
        self.assertEqual(self.jobs["android"]["strategy"]["matrix"]["target"], ["android", "android-armv7"])
        macos = [(r["target"], r["runner"]) for r in self.jobs["macos"]["strategy"]["matrix"]["include"]]
        self.assertEqual(macos, [("macos", "macos-15"), ("macos-intel", "macos-15-intel")])
        rows = {p["target"]: p["github"] for p in ci.PLATFORMS}
        for target, runner in unsigned + macos:
            self.assertEqual(rows[target], runner)
        self.assertEqual(sorted(ci.RELEASE_TARGETS), sorted([t for t, _ in unsigned + macos] + ["android", "android-armv7"]))

    def test_same_bootstrap_as_build_only_ci(self):
        for job in ["unsigned", "android", "macos"]:
            self.assertIn("python scripts/ci/frontend.py bootstrap", [s.get("run") for s in self.jobs[job]["steps"]])

    def test_publish_requires_every_platform_and_macos_skip_only_when_asked(self):
        publish = self.jobs["publish"]
        self.assertEqual(publish["needs"], ["version", "unsigned", "android", "macos"])
        condition = publish["if"]
        for job in ["version", "unsigned", "android"]:
            self.assertIn(f"needs.{job}.result == 'success'", condition)
        self.assertIn("(inputs.skip_macos && needs.macos.result == 'skipped')", condition)
        self.assertEqual(self.jobs["macos"]["if"],
                         "${{ !inputs.skip_macos && needs.version.outputs.guarded == 'true' }}")
        self.assertIn("needs.version.outputs.guarded == 'true'", condition)
        script = self.step("publish", "Create or update the GitHub Release")["run"]
        self.assertIn('--title "$TITLE"', script)
        self.assertIn("--generate-notes", script)
        self.assertIn("docs/release-notes/$TAG.md", script)
        self.assertIn("--verify-tag", script)
        sums = self.step("publish", "Check the asset set and write SHA256SUMS")["run"]
        self.assertIn("release-checksums", sums)

    def test_guard_runs_first_on_full_history_and_gates_every_secret_job(self):
        version = self.jobs["version"]
        self.assertEqual(version["steps"][0]["with"]["fetch-depth"], 0)
        runs = [s.get("run") for s in version["steps"]]
        guard = runs.index("python scripts/ci/frontend.py release-guard")
        self.assertLess(guard, runs.index('python scripts/ci/frontend.py release-version --tag "$RELEASE_TAG"'))
        self.assertEqual(version["steps"][guard]["id"], "guard")
        self.assertEqual(version["outputs"]["guarded"], "${{ steps.guard.outputs.guarded }}")
        self.assertEqual(secrets_in(version), set())
        for name in [*SIGNING_JOBS, "publish"]:
            with self.subTest(job=name):
                job = self.jobs[name]
                self.assertIn("needs.version.outputs.guarded == 'true'", job["if"])
                self.assertEqual(job["environment"], "release")
        self.assertNotIn("environment", self.jobs["unsigned"])
        self.assertNotIn("environment", self.jobs["version"])

    def test_store_jobs_skip_without_secrets_and_never_gate_the_release(self):
        self.assertEqual(self.jobs["publish"]["needs"], ["version", "unsigned", "android", "macos"])
        for name, store in [("testflight", "testflight"), ("play", "play")]:
            with self.subTest(job=name):
                job = self.jobs[name]
                self.assertEqual(job["needs"], "version")
                self.assertNotIn("continue-on-error", job)
                steps = job["steps"]
                check = next(i for i, s in enumerate(steps) if s.get("id") == "store")
                self.assertEqual(steps[check]["run"], f"python scripts/ci/frontend.py release-store-check --store {store}")
                for step in steps[check + 1:-1]:
                    self.assertEqual(step.get("if"), "steps.store.outputs.enabled == 'true'", step)
                for step in steps[:check]:
                    self.assertEqual(secrets_in(step), set())
                runs = [s.get("run") for s in steps]
                order = ["bootstrap", "release-store-build", "release-store-verify", "release-store-upload"]
                indices = [runs.index(f"python scripts/ci/frontend.py {action}") for action in order]
                self.assertEqual(indices, sorted(indices))
                # Store jobs publish nothing as workflow artifacts.
                self.assertFalse([s for s in steps if "upload-artifact" in s.get("uses", "")])
                upload = steps[runs.index("python scripts/ci/frontend.py release-store-upload")]
                self.assertEqual(secrets_in(upload),
                                 {"PLAY_SERVICE_ACCOUNT_JSON_B64"} if name == "play"
                                 else {"APPLE_NOTARY_KEY_P8_B64", "APPLE_NOTARY_KEY_ID", "APPLE_NOTARY_ISSUER_ID"})
        self.assertEqual(self.jobs["testflight"]["runs-on"], "macos-15")
        self.assertEqual(self.jobs["testflight"]["env"]["CI_FRONTEND_TARGET"], "ios")
        self.assertEqual(self.jobs["play"]["env"]["CI_FRONTEND_TARGET"], "android")
        self.assertIn("python scripts/ci/frontend.py release-ios-signing", [s.get("run") for s in self.jobs["testflight"]["steps"]])
        self.assertIn("python scripts/ci/frontend.py release-android-signing", [s.get("run") for s in self.jobs["play"]["steps"]])

    def test_dry_run_signs_and_keeps_artifacts_but_publishes_nothing(self):
        dry = self.workflow["on"]["workflow_dispatch"]["inputs"]["dry_run"]
        self.assertEqual((dry["type"], dry["default"]), ("boolean", True))
        # Tag pushes have no inputs, so !inputs.dry_run is true and they publish.
        self.assertIn("!inputs.dry_run", self.jobs["publish"]["if"])
        for name in STORE_JOBS:
            self.assertTrue(self.jobs[name]["if"].startswith("${{ !inputs.dry_run && "), name)
        for name in ["android", "macos", "unsigned"]:
            self.assertNotIn("dry_run", json.dumps(self.jobs[name]), name)
        job = self.jobs["dry-run"]
        self.assertEqual(job["needs"], ["version", "unsigned", "android", "macos"])
        self.assertIn("inputs.dry_run &&", job["if"])
        self.assertNotIn("!inputs.dry_run", job["if"])
        self.assertNotIn("permissions", job)
        self.assertNotIn("environment", job)
        self.assertEqual(secrets_in(job), set())
        raw = json.dumps(job)
        for forbidden in ["gh release", "GH_TOKEN", "release-store-upload", "upload-artifact"]:
            self.assertNotIn(forbidden, raw)
        self.assertIn("release-checksums", raw)

    def test_build_only_ci_still_has_no_production_credentials(self):
        raw = (ROOT / ".github/workflows/platforms.yml").read_text()
        self.assertNotIn("secrets.", raw)
        self.assertNotIn("release-", raw)
        self.assertNotIn("--final", raw)


class ReleaseDriver(unittest.TestCase):
    def fixture_root(self, version="0.3.1", build="36"):
        directory = tempfile.TemporaryDirectory()
        root = Path(directory.name)
        (root / "Cargo.toml").write_text(f'[package]\nname = "x"\nversion = "{version}"\n')
        (root / "Dioxus.toml").write_text(f'[application]\nname = "x"\nbuild = "{build}"\n')
        return directory, root

    def test_tag_must_match_cargo_version_and_dioxus_build(self):
        directory, root = self.fixture_root()
        with directory, patch.object(ci, "ROOT", root):
            info = ci.release_version("v0.3.1b36")
            self.assertEqual(info["name"], "Version 0.3.1 Build 36")
            self.assertEqual(info["version_code"], 301036)
            for tag in ["v0.3.1b35", "v0.3.2b36", "0.3.1b36", "v0.3.1", "v0.3.1b36-rc", "v0.3.1b36\n"]:
                with self.subTest(tag=tag), self.assertRaises(RuntimeError):
                    ci.release_version(tag)

    def test_repository_version_is_a_valid_release(self):
        cargo = ci.tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
        build = ci.tomllib.loads((ROOT / "Dioxus.toml").read_text())["application"]["build"]
        self.assertEqual(ci.release_version(f"v{cargo}b{build}")["build"], str(build))

    def test_release_commands_sign_only_android_and_macos_with_final(self):
        for name in ci.RELEASE_TARGETS:
            command = ci.release_command(name)
            self.assertEqual(command[:5], [sys.executable, "build.py", "--target", name, "--release"])
            self.assertEqual(command[5:], ["--final"] if name in ci.SIGNED_RELEASE_TARGETS else ["--no-sign"])
        for name in ["web", "ios", "watchos", "wearos", "android_sim"]:
            with self.assertRaises(ValueError):
                ci.release_command(name)

    def test_asset_names_match_previous_releases(self):
        # v0.3.1b35 published these exact names.
        previous = {"SEDS.GS.apk", "UBSEDS.GroundStation.Installer.exe", "UBSEDS.GS.dmg",
                    "UBSEDS.GS_x86_64.AppImage", "UBSEDS.GS_x86_64.flatpak"}
        names = {asset for assets in ci.RELEASE_ASSETS.values() for asset in assets.values()}
        self.assertLessEqual(previous, names)
        self.assertEqual(len(names), sum(len(a) for a in ci.RELEASE_ASSETS.values()), "asset names collide")
        for assets in ci.RELEASE_ASSETS.values():
            for source, asset in assets.items():
                self.assertNotIn(" ", asset)
                self.assertFalse(asset.lower().endswith(ci.SIGNING_MATERIAL_SUFFIXES))
        self.assertEqual(set(ci.RELEASE_ASSETS), set(ci.RELEASE_TARGETS))

    def test_release_env_passes_only_this_platforms_signing_variables(self):
        with tempfile.TemporaryDirectory() as temp:
            keystore = Path(temp) / "seds-release-signing" / "upload.jks"
            values = {"RUNNER_TEMP": temp, "ANDROID_KEYSTORE_PATH": str(keystore),
                      "ANDROID_KEYSTORE_PASSWORD": "store-fixture", "ANDROID_KEY_ALIAS": "upload",
                      "MACOS_SIGNING_IDENTITY": "Developer ID Application: X (T)", "NOTARY_PROFILE": "p",
                      "NOTARY_KEYCHAIN": "/k", "APPLE_ID": "fixture", "MACOS_DEVELOPER_ID_P12_PASSWORD": "fixture",
                      "ANDROID_UPLOAD_KEYSTORE_B64": "Zml4dHVyZQ==", "SEDS_GS_CI_ANDROID_KEYSTORE": "/dev"}
            with patch.dict(os.environ, values):
                linux = ci.release_env("linux")
                android = ci.release_env("android")
                macos = ci.release_env("macos")
        signing = ("ANDROID_KEY", "ANDROID_UPLOAD", "NOTARY_", "MACOS_", "APPLE_", "SEDS_GS_CI_", "SEDS_GS_NOTARY")
        self.assertFalse([k for k in linux if k.startswith(signing) or k == "APPLE_ID"])
        self.assertEqual({k for k in android if k.startswith(signing) or k == "APPLE_ID"},
                         {"ANDROID_KEYSTORE_PATH", "ANDROID_KEYSTORE_PASSWORD", "ANDROID_KEY_ALIAS"})
        self.assertEqual({k for k in macos if k.startswith(signing) or k == "APPLE_ID"},
                         {"MACOS_SIGNING_IDENTITY", "NOTARY_PROFILE", "NOTARY_KEYCHAIN", "SEDS_GS_NOTARY_RECEIPT"})
        self.assertEqual(android["RUSTFLAGS"], "-D warnings")

    def test_missing_production_signing_fails_instead_of_falling_back(self):
        with tempfile.TemporaryDirectory() as temp, patch.dict(os.environ, {"RUNNER_TEMP": temp}, clear=True):
            for name in ["android", "android-armv7", "macos", "macos-intel"]:
                with self.subTest(target=name), self.assertRaisesRegex(RuntimeError, "not prepared"):
                    ci.release_env(name)
            with self.assertRaisesRegex(RuntimeError, "ANDROID_UPLOAD_KEYSTORE_B64, ANDROID_KEYSTORE_PASSWORD"):
                ci.prepare_android_release_signing()
            with self.assertRaisesRegex(RuntimeError, "MACOS_DEVELOPER_ID_P12_B64"):
                ci.prepare_macos_release_signing()
            self.assertFalse((Path(temp) / "seds-release-signing").exists())
        with tempfile.TemporaryDirectory() as temp, patch.dict(os.environ, {
                "RUNNER_TEMP": temp, "ANDROID_KEYSTORE_PATH": "/elsewhere/upload.jks",
                "ANDROID_KEYSTORE_PASSWORD": "fixture"}, clear=True):
            with self.assertRaisesRegex(RuntimeError, "job-local"):
                ci.release_env("android")

    def test_notarization_prefers_api_key_and_rejects_partial_sets(self):
        api = {"APPLE_NOTARY_KEY_P8_B64": "a", "APPLE_NOTARY_KEY_ID": "b", "APPLE_NOTARY_ISSUER_ID": "c"}
        apple = {"APPLE_ID": "a", "APPLE_APP_SPECIFIC_PASSWORD": "b", "APPLE_TEAM_ID": "c"}
        for values, expected in [({**api, **apple}, "api-key"), (api, "api-key"), (apple, "apple-id")]:
            with patch.dict(os.environ, values, clear=True):
                self.assertEqual(ci.notary_mode(), expected)
        for values in [{}, {"APPLE_NOTARY_KEY_ID": "b"}, {"APPLE_ID": "a"}, {**apple, "APPLE_NOTARY_KEY_ID": "b"}]:
            with self.subTest(values=sorted(values)), patch.dict(os.environ, values, clear=True):
                with self.assertRaises(RuntimeError):
                    ci.notary_mode()

    def test_notarization_error_names_the_api_key_set_first(self):
        for values in [{}, {"APPLE_TEAM_ID": "T"}, {"APPLE_ID": "a", "APPLE_TEAM_ID": "T"}]:
            with self.subTest(values=sorted(values)), patch.dict(os.environ, values, clear=True):
                with self.assertRaises(RuntimeError) as raised:
                    ci.notary_mode()
                message = str(raised.exception)
                self.assertIn("APPLE_NOTARY_KEY_P8_B64, APPLE_NOTARY_KEY_ID and APPLE_NOTARY_ISSUER_ID", message)
                self.assertLess(message.index("APPLE_NOTARY_KEY_P8_B64"), message.index("Fallback"))
                for name in ci.NOTARY_APPLE_ID_SECRETS:
                    self.assertEqual(name in message.split("Fallback", 1)[1], name not in values, name)

    def test_android_keystore_is_private_outside_checkout_and_password_not_in_argv(self):
        keystore_bytes = b"fixture keystore bytes"
        with tempfile.TemporaryDirectory() as temp, tempfile.NamedTemporaryFile() as env_file:
            calls = []
            def quiet(argv, label, *, env=None, capture=False):
                calls.append((argv, env))
                return b"certificate-der"
            with patch.dict(os.environ, {"RUNNER_TEMP": temp, "GITHUB_ENV": env_file.name,
                                         "ANDROID_UPLOAD_KEYSTORE_B64": base64.b64encode(keystore_bytes).decode() + "\n",
                                         "ANDROID_KEYSTORE_PASSWORD": "store-fixture-secret"}, clear=True), \
                 patch.object(ci, "quiet_run", side_effect=quiet), contextlib.redirect_stdout(io.StringIO()):
                ci.prepare_android_release_signing()
            keystore = Path(temp) / "seds-release-signing/upload.jks"
            self.assertEqual(keystore.read_bytes(), keystore_bytes)
            if os.name != "nt":
                self.assertEqual(keystore.stat().st_mode & 0o777, 0o600)
                self.assertEqual(keystore.parent.stat().st_mode & 0o777, 0o700)
            self.assertNotIn("store-fixture-secret", repr([argv for argv, _ in calls]))
            self.assertEqual(calls[0][0][-4:], ["-alias", "upload", "-storepass:env", "SEDS_GS_STOREPASS"])
            self.assertEqual(calls[0][1]["SEDS_GS_STOREPASS"], "store-fixture-secret")
            exported = Path(env_file.name).read_text()
            self.assertNotIn("store-fixture-secret", exported)
            self.assertIn(f"ANDROID_KEYSTORE_PATH={keystore}", exported)
            self.assertIn("SEDS_GS_RELEASE_ANDROID_CERT_SHA256=" + hashlib.sha256(b"certificate-der").hexdigest(), exported)
            with patch.dict(os.environ, {"RUNNER_TEMP": temp}), contextlib.redirect_stdout(io.StringIO()):
                ci.release_cleanup()
            self.assertFalse(keystore.parent.exists())

    def test_signing_dir_refuses_the_checkout(self):
        with patch.dict(os.environ, {"RUNNER_TEMP": str(ROOT / "target")}):
            with self.assertRaisesRegex(RuntimeError, "outside the checkout"):
                ci.release_signing_dir()
        with patch.dict(os.environ, {}, clear=True), self.assertRaises(RuntimeError):
            ci.release_signing_dir()

    def test_cleanup_removes_generated_gradle_signing_config(self):
        with tempfile.TemporaryDirectory() as temp, tempfile.TemporaryDirectory() as runner:
            root = Path(temp)
            gradle = root / "target/dx/groundstation_frontend/release/android/app/app/build.gradle.kts"
            gradle.parent.mkdir(parents=True)
            gradle.write_text('storePassword = "fixture"\n')
            other = gradle.with_name("settings.gradle.kts")
            other.write_text("rootProject.name = \"app\"\n")
            with patch.object(ci, "ROOT", root), patch.dict(os.environ, {"RUNNER_TEMP": runner}), \
                 contextlib.redirect_stdout(io.StringIO()):
                ci.release_cleanup()
            self.assertFalse(gradle.exists())
            self.assertTrue(other.exists())

    def stage_fixture(self, root, name, extra=()):
        dist = root / "artifacts" / name / "dist"
        dist.mkdir(parents=True)
        for source in list(ci.RELEASE_ASSETS[name]) + list(extra):
            (dist / source).write_bytes(source.encode())
        with patch.object(ci, "ROOT", root), patch.dict(os.environ, {}, clear=True), \
             contextlib.redirect_stdout(io.StringIO()):
            ci.release_stage(name)
        return root / "release-assets" / name

    def test_stage_copies_only_named_assets_never_aab_or_keys(self):
        with tempfile.TemporaryDirectory() as temp:
            out = self.stage_fixture(Path(temp), "android", extra=["SEDS GS.aab", "upload.jks"])
            self.assertEqual(sorted(p.name for p in out.iterdir()), ["SEDS.GS.apk"])

    def test_stage_fails_when_an_expected_asset_is_missing(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "artifacts/linux/dist").mkdir(parents=True)
            (root / "artifacts/linux/dist/UBSEDS GS_x86_64.AppImage").write_text("x")
            with patch.object(ci, "ROOT", root), self.assertRaisesRegex(RuntimeError, "missing"):
                ci.release_stage("linux")

    def test_checksums_cover_exact_asset_set_and_refuse_signing_material(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            merged = root / "merged"
            merged.mkdir()
            for name in ["windows", "android"]:
                for asset in ci.RELEASE_ASSETS[name].values():
                    (merged / asset).write_text(asset)
            sums = ci.release_checksums(merged, ["windows", "android"])
            lines = sums.read_text().splitlines()
            self.assertEqual([l.split("  ")[1] for l in lines], ["SEDS.GS.apk", "UBSEDS.GroundStation.Installer.exe"])
            self.assertEqual(lines[0].split("  ")[0], hashlib.sha256(b"SEDS.GS.apk").hexdigest())
            sums.unlink()
            with self.assertRaisesRegex(RuntimeError, "differ"):
                ci.release_checksums(merged, ["windows", "android", "macos"])
            (merged / "upload.p12").write_text("x")
            with self.assertRaisesRegex(RuntimeError, "signing material"):
                ci.release_checksums(merged, ["windows", "android"])

    def test_apksigner_scheme_labelled_output(self):
        # build-tools 37 labels signers by scheme (the release dry run of 2026-11-24).
        digest = "7e1a7dedfd1fe4ae0dc356c9e43f4434bcd469cc2ee04daa6d54fb267c213f8f"
        dn = "CN=Example Owner, OU=Example, O=Example, L=Town, ST=New York, C=US"
        output = ("Verifies\nVerified using v3 scheme (APK Signature Scheme v3): true\n"
                  "Verified using v3.1 scheme (APK Signature Scheme v3.1): false\nNumber of signers: 1\n"
                  f"V3.0 Signer: certificate DN: {dn}\n"
                  f"V3.0 Signer: certificate SHA-256 digest: {digest}\n"
                  "V3.0 Signer: certificate SHA-1 digest: ca804300b5051749ccc4496923009d34feac3357\n"
                  "V3.0 Signer: public key SHA-256 digest: e64305af7b2ad8154e635c01b0d4388ffb153d1d0ab2fe7cd6d04ca4c3aba98a\n")
        self.assertEqual(ci.apk_signers(output), [(dn, digest)])
        # The same signer listed under two schemes is still one signer.
        both = output + f"Signer #1 certificate DN: {dn}\nSigner #1 certificate SHA-256 digest: {digest}\n"
        self.assertEqual(ci.apk_signers(both), [(dn, digest)])
        # A second, different signer is two signers and does not satisfy a count of 1.
        other = "0" * 64
        with self.assertRaises(RuntimeError):
            ci.apk_signers(output + f"V3.1 Signer #2: certificate DN: CN=Other\nV3.1 Signer #2: certificate SHA-256 digest: {other}\n")
        # A DN without its digest (or the reverse) is refused.
        with self.assertRaises(RuntimeError):
            ci.apk_signers(output.replace(f"V3.0 Signer: certificate SHA-256 digest: {digest}\n", ""))

    def test_apksigner_output_parsing_and_production_match(self):
        digest = "41be92372808d89ec0013d89b52e203d8d5aa6fd0aa7f840383929bcd7878f0e"
        output = ("Verifies\nVerified using v1 scheme (JAR signing): false\n"
                  "Verified using v2 scheme (APK Signature Scheme v2): true\n"
                  "Verified using v3 scheme (APK Signature Scheme v3): true\nNumber of signers: 1\n"
                  "Signer #1 certificate DN: CN=Example, O=UB SEDS\n"
                  f"Signer #1 certificate SHA-256 digest: {digest}\n")
        self.assertEqual(ci.apk_signers(output), [("CN=Example, O=UB SEDS", digest)])
        with self.assertRaises(RuntimeError):
            ci.apk_signers(output.replace("Number of signers: 1", "Number of signers: 2"))
        self.assertEqual(ci.normalized_digest(ci.colon_hex(digest)), digest)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            apk = root / "artifacts/android/dist/SEDS GS.apk"
            apk.parent.mkdir(parents=True)
            apk.write_bytes(b"apk")
            result = subprocess.CompletedProcess([], 0, stdout=output, stderr="")
            for env, ok in [({"SEDS_GS_RELEASE_ANDROID_CERT_SHA256": digest}, True),
                            ({"SEDS_GS_RELEASE_ANDROID_CERT_SHA256": "0" * 64}, False),
                            ({}, False),
                            ({"SEDS_GS_RELEASE_ANDROID_CERT_SHA256": digest, "ANDROID_EXPECTED_CERT_SHA256": ci.colon_hex(digest)}, True),
                            ({"SEDS_GS_RELEASE_ANDROID_CERT_SHA256": digest, "ANDROID_EXPECTED_CERT_SHA256": "1" * 64}, False)]:
                with self.subTest(env=sorted(env)), patch.object(ci, "ROOT", root), \
                     patch.object(ci, "find_apksigner", return_value="apksigner"), \
                     patch.object(ci.subprocess, "run", return_value=result), \
                     patch.dict(os.environ, env, clear=True), contextlib.redirect_stdout(io.StringIO()):
                    if ok:
                        ci.verify_android_release("android")
                    else:
                        with self.assertRaises(RuntimeError):
                            ci.verify_android_release("android")

    def test_github_env_values_are_single_line(self):
        with tempfile.NamedTemporaryFile() as env_file, patch.dict(os.environ, {"GITHUB_ENV": env_file.name}):
            with self.assertRaises(RuntimeError):
                ci.github_file("GITHUB_ENV", {"A": "x\nB=y"})
            self.assertEqual(Path(env_file.name).read_text(), "")

    def test_build_only_env_drops_release_secrets(self):
        names = ["ANDROID_UPLOAD_KEYSTORE_B64", "ANDROID_KEYSTORE_PASSWORD", "MACOS_DEVELOPER_ID_P12_B64",
                 "MACOS_SIGNING_IDENTITY", "APPLE_NOTARY_KEY_P8_B64", "APPLE_ID", "APPLE_APP_SPECIFIC_PASSWORD",
                 "APPLE_TEAM_ID", "SEDS_GS_RELEASE_ANDROID_CERT_SHA256", "NOTARY_KEYCHAIN"]
        with patch.dict(os.environ, {name: "fixture" for name in names}):
            env = ci.ci_env()
        self.assertFalse(set(names) & set(env))


if __name__ == "__main__":
    unittest.main()

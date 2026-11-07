"""Hosted Android output regressions; fake packaging plus one real PATH process.

These contracts do not compile Rust, Gradle, Android code, or launch a device.
"""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module

build = load("android_packaging_build", "build.py")
ci = load("android_packaging_ci", "scripts/ci/frontend.py")
KEY = "SEDS_GS_CI_ANDROID_KEYSTORE"
PASSWORD = "SEDS_GS_CI_ANDROID_KEYSTORE_PASSWORD_FILE"

class AndroidPackagingContracts(unittest.TestCase):
    def test_canonical_artifacts_are_idempotent_recognized_inputs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dist = root / "dist"
            dist.mkdir()
            for name in ["SEDS GS.apk", "SEDS GS.aab"]:
                (dist / name).write_bytes(name.encode())
            before = {p.name: p.read_bytes() for p in dist.iterdir()}
            errors = io.StringIO()
            with contextlib.redirect_stderr(errors):
                build.rename_android_artifacts(root)
                build.rename_android_artifacts(root)
            self.assertEqual(errors.getvalue(), "")
            self.assertEqual(before, {p.name: p.read_bytes() for p in dist.iterdir()})

    def test_legacy_names_still_rename_and_missing_artifacts_still_warn(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dist = root / "dist"
            dist.mkdir()
            (dist / "GroundstationFrontend-aarch64-linux-android.aab").write_bytes(b"bundle")
            build.rename_android_artifacts(root)
            self.assertEqual((dist / "SEDS GS.aab").read_bytes(), b"bundle")
            (dist / "SEDS GS.aab").unlink()
            (dist / "unrelated.apk").write_bytes(b"other")
            errors = io.StringIO()
            with contextlib.redirect_stderr(errors):
                build.rename_android_artifacts(root)
            self.assertIn("Warning: no android artifacts matched", errors.getvalue())

    def package_fixture(self, root, environment):
        dist = root / "dist"
        dist.mkdir()
        (dist / "SEDS GS.aab").write_bytes(b"inert bundle fixture")
        calls = []
        def fake_run(argv, **kwargs):
            calls.append(argv)
            output, = [p.removeprefix("--output=") for p in argv if p.startswith("--output=")]
            with zipfile.ZipFile(output, "w") as archive:
                archive.writestr("universal.apk", b"inert apk fixture")
        with patch.dict(os.environ, environment, clear=True), \
             patch.object(build, "_resolve_bundletool_command", return_value=["java", "-jar", "fixture.jar"]), \
             patch.object(build, "run", side_effect=fake_run):
            result = build.build_android_universal_apk(root)
        self.assertEqual(result.read_bytes(), b"inert apk fixture")
        return calls[0]

    def test_ci_apk_passes_explicit_key_and_password_environment_references(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            key = root / "development.p12"
            key.write_bytes(b"fixture key, not a real credential")
            secret = "fixture-password-not-for-log"
            password_file = root / "password.txt"
            password_file.write_text(secret)
            argv = self.package_fixture(root, {KEY: str(key), PASSWORD: str(password_file)})
            self.assertIn("--ks=" + str(key), argv)
            self.assertIn("--ks-key-alias=androiddebugkey", argv)
            self.assertIn("--ks-pass=file:" + str(password_file), argv)
            self.assertIn("--key-pass=file:" + str(password_file), argv)
            self.assertNotIn(secret, str(argv))

    def test_local_bundletool_arguments_remain_unchanged(self):
        with tempfile.TemporaryDirectory() as temporary:
            argv = self.package_fixture(Path(temporary), {"ANDROID_KEYSTORE_PASSWORD": "local-fixture-only"})
            self.assertFalse(any(p.startswith("--ks") or p.startswith("--key-pass") for p in argv))

    def test_partial_ci_configuration_fails_before_external_bundletool(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "dist").mkdir()
            (root / "dist/SEDS GS.aab").write_bytes(b"bundle")
            with patch.dict(os.environ, {KEY: "/unusable-fixture-key"}, clear=True), \
                 patch.object(build, "_resolve_bundletool_command", return_value=["bundletool"]), \
                 patch.object(build, "run") as run:
                with self.assertRaisesRegex(RuntimeError, "incompletely configured"):
                    build.build_android_universal_apk(root)
                run.assert_not_called()

    def test_ci_never_consults_local_upload_identity_and_local_path_is_preserved(self):
        with patch.dict(os.environ, {KEY: "/ci-owned/key"}, clear=True), \
             patch.object(build, "_android_signing_settings", side_effect=AssertionError("local identity read")):
            self.assertFalse(build._configure_android_signing(Path("/unused"), Path("/unused")))
        with patch.dict(os.environ, {}, clear=True), \
             patch.object(build, "_android_signing_settings", side_effect=ValueError("original local settings")):
            with self.assertRaisesRegex(ValueError, "original local settings"):
                build._configure_android_signing(Path("/unused"), Path("/unused"))

    def test_ci_env_removes_release_and_stale_ci_signing_material(self):
        with patch.dict(os.environ, {"ANDROID_KEYSTORE_PASSWORD": "private-fixture", KEY: "/stale/key", PASSWORD: "stale-fixture"}, clear=True):
            environment = ci.ci_env()
        self.assertNotIn("ANDROID_KEYSTORE_PASSWORD", environment)
        self.assertNotIn(KEY, environment)
        self.assertNotIn(PASSWORD, environment)

    def test_fresh_key_is_private_and_password_is_never_in_argv_or_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / ".ci-tools").mkdir()
            (root / "ci-results").mkdir()
            environment = {"PATH": "/fixture"}
            seen = []
            def fake_checked(argv, env, label):
                seen.append((argv, label))
                Path(argv[argv.index("-keystore") + 1]).write_bytes(b"inert development key fixture")
            with patch.object(ci, "ROOT", root), patch.object(ci, "checked", side_effect=fake_checked):
                ci.prepare_android_development_signing(environment)
                with self.assertRaises(FileExistsError):
                    ci.prepare_android_development_signing(environment)
            key = Path(environment[KEY])
            self.assertEqual(key.stat().st_mode & 0o777, 0o600)
            self.assertEqual(key.parent.stat().st_mode & 0o777, 0o700)
            password_file = Path(environment[PASSWORD])
            self.assertEqual(password_file.stat().st_mode & 0o777, 0o600)
            secret = password_file.read_text().strip()
            self.assertNotIn(secret, str(seen))
            receipt = (root / "ci-results/android-development-signing.json").read_text()
            self.assertNotIn(secret, receipt)
            self.assertFalse(json.loads(receipt)["production_credentials_used"])
            self.assertFalse(json.loads(receipt)["keystore_uploaded"])

    def test_both_android_profiles_label_actual_nonempty_build_outputs(self):
        for profile in ["debug", "release"]:
            with self.subTest(profile=profile), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                (root / ".ci-tools").mkdir()
                (root / "ci-results").mkdir()
                order = []
                def fake_checked(argv, env, label, **kwargs):
                    order.append(label)
                    if label == "android-development-key":
                        Path(argv[argv.index("-keystore") + 1]).write_bytes(b"inert key")
                    else:
                        self.assertIn(KEY, env)
                        output = root / "artifacts/android"
                        output.mkdir(parents=True)
                        (output / "fixture.apk").write_bytes(b"inert apk")
                with patch.object(ci, "ROOT", root), patch.object(ci, "validate_host"), \
                     patch.object(ci, "ci_env", return_value={"PATH": "/fixture"}), \
                     patch.object(ci, "verify_bundletool"), \
                     patch.object(ci.shutil, "which", return_value="/fixture/tool"), \
                     patch.object(ci, "checked", side_effect=fake_checked):
                    ci.run("android", profile)
                self.assertEqual(order, ["android-development-key", "build-android-" + profile])
                label = root / "artifacts/android/CI-DEVELOPMENT-SIGNED-NOT-FOR-DISTRIBUTION.txt"
                self.assertIn("Not a production release", label.read_text())
                manifest = json.loads((root / "ci-results/artifacts.json").read_text())
                self.assertIn("artifacts/android/" + label.name, manifest)
                self.assertFalse(any(".p12" in path for path in manifest))

    @unittest.skipUnless(os.name == "posix", "Container PATH execution contract requires POSIX")
    def test_actual_wasm_opt_probe_preserves_prepared_path_without_login_shell(self):
        with tempfile.TemporaryDirectory(prefix="host tool path ") as temporary:
            root = Path(temporary)
            executable = root / "wasm-opt"
            record = root / "observed.json"
            executable.write_text("#!/usr/bin/python3\nimport json,os,sys\nfrom pathlib import Path\nPath(os.environ['SENTINEL_RECORD']).write_text(json.dumps({'argv':sys.argv[1:],'path':os.environ['PATH']}))\nprint('wasm-opt version 133')\n")
            executable.chmod(0o700)
            build._verify_container_wasm_opt(root, {"PATH": str(root), "SENTINEL_RECORD": str(record)})
            observed = json.loads(record.read_text())
            self.assertEqual(observed, {"argv": ["--version"], "path": str(root)})

    def failing_process(self, root, program, *, environment=None, log=None):
        with contextlib.redirect_stdout(io.StringIO()), patch.object(build, "LOG_FILE", log):
            with self.assertRaises(build.CapturedCommandFailure) as caught:
                build.run_filtered([sys.executable, "-c", program], root, env=environment)
        return caught.exception

    def test_actual_rust_failure_rejects_stale_gradle_project(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "app/src/main").mkdir(parents=True)
            failure = self.failing_process(root, "print('error[E0080]: compile_error!(Only 64-bit Android targets are supported)'); raise SystemExit(1)")
            self.assertTrue(failure.rust_compile_failed)
            self.assertFalse(build._android_gradle_fallback_allowed(failure, root))
            self.assertEqual(failure.returncode, 1)

    def test_rust_failure_survives_bounded_tail_and_log_filter(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "app/src/main").mkdir(parents=True)
            program = "print('error[E0001]: original Rust failure'); [print('padding ' + 'x'*5000) for _ in range(140)]; print('FAILURE: Build failed with an exception.'); print('Execution failed for task :app:packageDebug'); raise SystemExit(3)"
            failure = self.failing_process(root, program, log=root / "current.log")
            self.assertNotIn("original Rust failure", failure.output)
            self.assertLessEqual(len(failure.output), 128 * 4096)
            self.assertTrue(failure.rust_compile_failed)
            self.assertFalse(build._android_gradle_fallback_allowed(failure, root))

    def test_actual_current_gradle_failure_requires_staged_project(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            program = "print('FAILURE: Build failed with an exception.'); print('Execution failed for task :app:packageDebug'); raise SystemExit(1)"
            failure = self.failing_process(root, program)
            self.assertFalse(build._android_gradle_fallback_allowed(failure, root))
            (root / "app/src/main").mkdir(parents=True)
            self.assertTrue(build._android_gradle_fallback_allowed(failure, root))
            self.assertFalse(failure.rust_compile_failed)

    def test_actual_unknown_empty_and_historical_failures_cannot_authorize_fallback(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "app/src/main").mkdir(parents=True)
            for program in ["raise SystemExit(1)", "print('unknown dx failure'); raise SystemExit(1)"]:
                failure = self.failing_process(root, program)
                self.assertFalse(build._android_gradle_fallback_allowed(failure, root))
            with patch.object(build, "_tail_log_text", return_value="FAILURE: Build failed with an exception. Execution failed for task :app:packageDebug"):
                self.assertFalse(build._android_gradle_fallback_allowed(subprocess.CalledProcessError(1, ["dx"]), root))

    def test_actual_failure_exception_redacts_credentials(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            secret = "inert-test-password-value"
            failure = self.failing_process(root, "import os; print(os.environ['TEST_PASSWORD']); print('Authorization: Bearer inert-token'); raise SystemExit(1)", environment={"TEST_PASSWORD": secret})
            self.assertNotIn(secret, failure.output)
            self.assertNotIn("inert-token", failure.output)
            self.assertIn("<redacted>", failure.output)

    def test_known_repair_branches_retry_and_propagate_retry_failure(self):
        for failure_text, reset_expected in [
            ("The contents of the immutable workspace were modified", True),
            ("wasm-bindgen version mismatch", False),
        ]:
            with self.subTest(failure=failure_text), tempfile.TemporaryDirectory() as temporary, contextlib.ExitStack() as stack:
                root = Path(temporary)
                stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
                for name in ["clear_generated_android_project", "clear_android_staging_artifacts"]:
                    stack.enter_context(patch.object(build, name))
                for name in ["is_container", "in_docker_build"]:
                    stack.enter_context(patch.object(build, name, return_value=False))
                stack.enter_context(patch.object(build, "_ensure_android_env", return_value={"PATH": "/fixture"}))
                ensure = stack.enter_context(patch.object(build, "_ensure_wasm_bindgen_cli", return_value=None))
                stack.enter_context(patch.object(build, "_find_dx", return_value=Path("/fixture/dx")))
                stack.enter_context(patch.object(build, "_installed_dx_version", return_value=None))
                stack.enter_context(patch.object(build, "_workspace_dioxus_versions", return_value=[]))
                stack.enter_context(patch.object(build, "_dx_version_line_filter", return_value=None))
                reset = stack.enter_context(patch.object(build, "_reset_android_gradle_home"))
                stack.enter_context(patch.object(build, "run_filtered", side_effect=build.CapturedCommandFailure(1, ["dx"], failure_text, False)))
                retry = stack.enter_context(patch.object(build, "run", side_effect=RuntimeError("retry failure must propagate")))
                with self.assertRaisesRegex(RuntimeError, "retry failure must propagate"):
                    build.build_frontend(root, "android", rust_target="aarch64-linux-android", debug_mode=True)
                self.assertEqual(reset.call_count, int(reset_expected))
                self.assertEqual(ensure.call_count, 1 if reset_expected else 2)
                self.assertEqual(retry.call_count, 1)
                self.assertEqual(retry.call_args.args[0][0], "/fixture/dx")

    @contextlib.contextmanager
    def native_build_fixture(self, root, executable):
        with contextlib.ExitStack() as stack:
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            stack.enter_context(contextlib.redirect_stderr(io.StringIO()))
            for name in ["_ensure_bundle_icon_compat", "_clear_dioxus_bundle_identity_cache", "clear_app_bundle",
                         "cleanup_linux_package_artifacts", "cleanup_linux_loose_binary_artifacts"]:
                stack.enter_context(patch.object(build, name))
            for name in ["is_container", "in_docker_build"]:
                stack.enter_context(patch.object(build, name, return_value=False))
            ensure = stack.enter_context(patch.object(build, "_ensure_wasm_bindgen_cli", return_value=None))
            stack.enter_context(patch.object(build, "_find_dx", return_value=executable))
            stack.enter_context(patch.object(build, "_installed_dx_version", return_value=None))
            stack.enter_context(patch.object(build, "_workspace_dioxus_versions", return_value=[]))
            stack.enter_context(patch.object(build, "_dx_version_line_filter", return_value=None))
            yield ensure

    def native_child(self, root, body):
        executable = root / "dx"
        executable.write_text("#!" + sys.executable + "\nfrom pathlib import Path\n" + body)
        executable.chmod(0o700)
        return executable

    @unittest.skipUnless(os.name == "posix", "Actual native child executable contract requires POSIX")
    def test_actual_nonandroid_current_command_repairs_without_global_log(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = self.native_child(root, "p=Path('calls'); n=int(p.read_text()) if p.exists() else 0; p.write_text(str(n+1))\nif n==0:\n print('wasm-bindgen version mismatch'); raise SystemExit(1)\n")
            with self.native_build_fixture(root, executable) as ensure, \
                 patch.object(build, "_tail_log_text", side_effect=AssertionError("historical logs must not be read")), \
                 patch.object(build, "rename_macos_app_bundle", side_effect=RuntimeError("repair completed")):
                with self.assertRaisesRegex(RuntimeError, "repair completed"):
                    build.build_frontend(root, "macos", rust_target="x86_64-apple-darwin", debug_mode=True)
                self.assertEqual(ensure.call_count, 2)
            self.assertEqual((root / "calls").read_text(), "2")

    @unittest.skipUnless(os.name == "posix", "Actual native child executable contract requires POSIX")
    def test_actual_nonandroid_empty_or_rust_failure_cannot_use_stale_logs(self):
        for body in ["raise SystemExit(7)\n", "print('error[E0001]: Rust compile failed'); raise SystemExit(7)\n"]:
            with self.subTest(body=body), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                executable = self.native_child(root, body)
                with self.native_build_fixture(root, executable) as ensure, \
                     patch.object(build, "_tail_log_text", return_value="wasm-bindgen version mismatch"), \
                     patch.object(build, "rename_macos_app_bundle") as package:
                    with self.assertRaises(SystemExit) as caught:
                        build.build_frontend(root, "macos", rust_target="x86_64-apple-darwin", debug_mode=True)
                    self.assertEqual(caught.exception.code, 7)
                    self.assertEqual(ensure.call_count, 1)
                    package.assert_not_called()

    @unittest.skipUnless(os.name == "posix", "Actual native child executable contract requires POSIX")
    def test_actual_linux_current_packaging_failure_keeps_staged_payload_repair(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = self.native_child(root, "print('Failed to bundle AppImage: linuxdeploy failed'); raise SystemExit(1)\n")
            stage = root / "target/dx/groundstation_frontend/debug/linux/app"
            stage.mkdir(parents=True)
            with self.native_build_fixture(root, executable), \
                 patch.object(build, "_target_root", return_value=root / "target"), \
                 patch.object(build, "_frontend_package_name", return_value="groundstation_frontend"), \
                 patch.object(build, "cleanup_linux_generated_package_artifacts"), \
                 patch.object(build, "build_manual_appimage", side_effect=RuntimeError("current payload repair")), \
                 patch.object(build, "_tail_log_text", side_effect=AssertionError("no stale global log")):
                with self.assertRaisesRegex(RuntimeError, "current payload repair"):
                    build.build_frontend(root, "linux", rust_target="x86_64-unknown-linux-gnu", debug_mode=True)

if __name__ == "__main__":
    unittest.main()

"""Fast source/behaviour contracts; these do not pretend to compile any platform."""
import contextlib
import io
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import yaml

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("frontend_ci", ROOT / "scripts/ci/frontend.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)
target_spec = importlib.util.spec_from_file_location("frontend_targets", ROOT / "scripts/build_targets.py")
targets = importlib.util.module_from_spec(target_spec)
import sys
sys.modules[target_spec.name] = targets
target_spec.loader.exec_module(targets)


class PlatformCI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.github = yaml.safe_load((ROOT / ".github/workflows/platforms.yml").read_text())
        cls.gitlab = yaml.safe_load((ROOT / ".gitlab-ci.yml").read_text())

    def test_inventory_exactly_covers_actual_public_build_targets(self):
        self.assertEqual([p["target"] for p in ci.PLATFORMS], list(targets.TARGETS))
        self.assertEqual(len(ci.PLATFORMS), 12)

    def test_github_both_profiles_and_no_fail_fast_hiding_platforms(self):
        strategy = self.github["jobs"]["build"]["strategy"]
        self.assertEqual(strategy["matrix"]["target"], list(targets.TARGETS))
        self.assertEqual(strategy["matrix"]["profile"], ["debug", "release"])
        self.assertIs(strategy["fail-fast"], False)
        self.assertEqual(len(strategy["matrix"]["target"]) * 2, 24)

    def test_github_native_runner_bindings(self):
        matrix = self.github["jobs"]["build"]["strategy"]["matrix"]
        self.assertEqual(matrix["include"], [{"target": p["target"], "runner": p["github"]} for p in ci.PLATFORMS])
        self.assertEqual(self.github["jobs"]["build"]["runs-on"], "${{ matrix.runner }}")

    def test_gitlab_has_all24_real_native_jobs_no_allow_failure(self):
        job = self.gitlab["frontend-build"]
        rows = job["parallel"]["matrix"]
        self.assertEqual([r["CI_FRONTEND_TARGET"] for r in rows], list(targets.TARGETS))
        self.assertTrue(all(r["CI_FRONTEND_PROFILE"] == ["debug", "release"] for r in rows))
        self.assertEqual(job["tags"], ["$UI_RUNNER"])
        self.assertNotIn("allow_failure", job)
        self.assertEqual([r["UI_RUNNER"] for r in rows], [p["gitlab"] for p in ci.PLATFORMS])

    def test_apple_windows_linux_wrong_host_is_error_not_skip(self):
        for name in ["ios", "ios_sim", "ios_sim_intel", "macos", "macos-intel", "windows", "linux-arm64"]:
            with self.subTest(target=name), patch.object(ci.platform, "system", return_value="Linux"), patch.object(ci.platform, "machine", return_value="x86_64"):
                with self.assertRaises(RuntimeError):
                    ci.validate_host(name)

    def test_all_declared_native_hosts_accepted(self):
        for p in ci.PLATFORMS:
            with self.subTest(target=p["target"]), patch.object(ci.platform, "system", return_value=p["host"]), patch.object(ci.platform, "machine", return_value=p["arch"]):
                self.assertEqual(ci.validate_host(p["target"]), (p["host"], p["arch"]))

    def test_arm_and_windows_machine_aliases(self):
        self.assertEqual(ci.normalized_arch("ARM64"), "aarch64")
        self.assertEqual(ci.normalized_arch("AMD64"), "x86_64")

    def test_exact_real_build_only_commands_all24(self):
        for p in ci.PLATFORMS:
            for profile in ["debug", "release"]:
                command = ci.build_command(p["target"], profile)
                self.assertEqual(command, [sys.executable, "build.py", "--target", p["target"], "--" + profile, "--no-sign"])
                self.assertTrue(targets.parse_args(command[2:]).no_sign)
                self.assertNotIn("--dry-run", command)
                self.assertNotIn("--final", command)

    def test_bad_profile_and_unknown_target_refused(self):
        with self.assertRaises(ValueError):
            ci.build_command("web", "production-ish")
        with self.assertRaises(StopIteration):
            ci.build_command("fake", "release")

    def test_unsigned_ios_dispatch_builds_real_target_without_packaging(self):
        for name in ["ios", "ios_sim", "ios_sim_intel"]:
            for profile in ["debug", "release"]:
                with self.subTest(target=name, profile=profile), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    build_app, package_ios, finalize = Mock(), Mock(), Mock()
                    with patch.object(targets.platform, "system", return_value="Darwin"), \
                         patch.object(targets.subprocess, "run") as command:
                        self.assertTrue(targets.handle_target_args(
                            ci.build_command(name, profile)[2:], root, build_app, package_ios, finalize))
                    command.assert_called_once_with(
                        ["rustup", "target", "add", targets.TARGETS[name].triple], cwd=root, check=True)
                    build_app.assert_called_once_with(
                        root, platform_name="ios", rust_target=targets.TARGETS[name].triple,
                        debug_mode=profile == "debug", max_size=False,
                        android_package_type="apk", sign_macos=False)
                    package_ios.assert_not_called()
                    finalize.assert_not_called()

    def test_tools_follow_resolved_lock_versions_not_manifest_lower_bounds(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)):
            lock = Path(directory) / "Cargo.lock"
            for wasm in ["0.2.130", "0.2.131"]:
                lock.write_text('[[package]]\nname = "wasm-bindgen"\nversion = "' + wasm +
                                '"\n[[package]]\nname = "dioxus"\nversion = "0.7.11"\n')
                self.assertEqual(ci.resolved_tool_versions(), {"dioxus": "0.7.11", "wasm-bindgen": wasm})
            lock.write_text('[[package]]\nname = "dioxus"\nversion = "0.7.10"\n')
            with self.assertRaises(RuntimeError):
                ci.resolved_tool_versions()
            lock.write_text('[[package]]\nname = "dioxus"\nversion = "0.7.10"\n'
                            '[[package]]\nname = "dioxus"\nversion = "0.7.11"\n')
            with self.assertRaises(RuntimeError):
                ci.resolved_tool_versions()

    def test_bootstrap_resolves_missing_lock_and_installs_exact_tools_unlocked(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
             patch.object(ci, "validate_host", return_value=("Linux", "x86_64")), \
             patch.object(ci.shutil, "which", return_value="fixture-tool"), \
             patch.object(ci, "install_binaryen"):
            calls = []
            def checked(argv, _env, label):
                calls.append(argv)
                if label == "resolve-application-lock":
                    (Path(directory) / "Cargo.lock").write_text(
                        '[[package]]\nname = "dioxus"\nversion = "0.7.10"\n'
                        '[[package]]\nname = "wasm-bindgen"\nversion = "0.2.130"\n')
            with patch.object(ci, "checked", side_effect=checked):
                ci.bootstrap("web")
                self.assertIn(["cargo", "generate-lockfile"], calls)
                self.assertIn(["cargo", "install", "dioxus-cli", "--version", "=0.7.10"], calls)
                self.assertIn(["cargo", "install", "wasm-bindgen-cli", "--version", "=0.2.130"], calls)
                self.assertFalse(any("--locked" in argv for argv in calls if argv[:2] == ["cargo", "install"]))
                receipt = json.loads((Path(directory) / "ci-results/resolved-tools.json").read_text())
                self.assertEqual(receipt["lock_sha256"], ci.file_sha256(Path(directory) / "Cargo.lock"))
                calls.clear()
                ci.bootstrap("web")
                self.assertNotIn(["cargo", "generate-lockfile"], calls)

    def test_warning_recognition_is_not_a_blanket_whitelist(self):
        for line in ["warning: unused item", "warning[dead_code]: item", "WARNING SDK mismatch", "WARN retry failed", "w: Kotlin warning", "\x1b[33mwarning: colorful\x1b[0m"]:
            self.assertTrue(ci.WARNING.search(ci.ANSI.sub("", line)), line)
        for line in ["Finished release profile", "warning_lines=0", "0 warnings"]:
            self.assertFalse(ci.WARNING.search(line), line)

    def test_real_subprocess_warning_exit0_still_fails_and_log_is_preserved(self):
        with tempfile.TemporaryDirectory() as root, patch.object(ci, "ROOT", Path(root)):
            captured = io.StringIO()
            with self.assertRaises(RuntimeError), contextlib.redirect_stdout(captured):
                ci.checked([sys.executable, "-c", "print('warning: causal fixture')"], os.environ.copy(), "warn")
            self.assertEqual(captured.getvalue(), "warning: causal fixture\n")
            row = json.loads((Path(root) / "ci-results/warn.json").read_text())
            self.assertEqual(row["exit"], 0)
            self.assertEqual(row["warning_lines"], 1)
            self.assertIn("causal fixture", (Path(root) / "ci-results/warn.log").read_text())

    def test_real_nonzero_subprocess_is_not_swallowed(self):
        with tempfile.TemporaryDirectory() as root, patch.object(ci, "ROOT", Path(root)):
            with self.assertRaises(RuntimeError):
                ci.checked([sys.executable, "-c", "raise SystemExit(7)"], os.environ.copy(), "fail")
            self.assertEqual(json.loads((Path(root) / "ci-results/fail.json").read_text())["exit"], 7)

    def test_real_clean_subprocess_is_accepted(self):
        with tempfile.TemporaryDirectory() as root, patch.object(ci, "ROOT", Path(root)):
            ci.checked([sys.executable, "-c", "print('build complete')"], os.environ.copy(), "ok")
            self.assertEqual(json.loads((Path(root) / "ci-results/ok.json").read_text())["warning_lines"], 0)

    def test_env_fatal_warnings_no_signing_credentials_or_unsafe_rustflags(self):
        with patch.dict(os.environ, {"RUSTFLAGS": "-A warnings", "ANDROID_KEYSTORE_PASSWORD": "fixture", "NOTARY_PASSWORD": "fixture", "IOS_DISTRIBUTION_MOBILEPROVISION": "fixture"}):
            env = ci.ci_env()
        self.assertEqual(env["RUSTFLAGS"], "-D warnings")
        self.assertEqual(env["RUSTDOCFLAGS"], "-D warnings")
        self.assertFalse(any(k.startswith(("NOTARY_", "ANDROID_KEY", "IOS_DISTRIBUTION_")) for k in env))
        self.assertEqual(env["CARGO_BUILD_JOBS"], "2")
        self.assertEqual(env["CARGO_INCREMENTAL"], "0")

    def test_encoded_and_target_rustflags_cannot_override_warning_policy(self):
        for name in ["CARGO_ENCODED_RUSTFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS",
                     "CARGO_TARGET_AARCH64_APPLE_IOS_RUSTFLAGS"]:
            with self.subTest(name=name), patch.dict(os.environ, {name: ""}):
                with self.assertRaisesRegex(RuntimeError, name):
                    ci.ci_env()

    def test_parsed_linux_install_script_is_two_valid_shell_commands(self):
        import shlex
        step = next(s for s in self.github["jobs"]["build"]["steps"]
                    if s.get("name") == "Linux native libraries")
        lines = [shlex.split(line) for line in step["run"].splitlines() if line.strip()]
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[0], ["sudo", "apt-get", "update"])
        self.assertEqual(lines[1][:4], ["sudo", "apt-get", "install", "-y"])
        for package in ["libgtk-3-dev", "libwebkit2gtk-4.1-dev", "librsvg2-dev", "libfuse2t64"]:
            self.assertIn(package, lines[1])

    def test_binaryen_all_real_host_assets_checksum_pinned(self):
        for p in ci.PLATFORMS:
            self.assertIn((p["host"], p["arch"]), ci.BINARYEN)
        for suffix, digest in ci.BINARYEN.values():
            self.assertEqual(len(digest), 64)
            self.assertNotIn("latest", suffix)

    def test_github_no_mutable_release_sign_upload_credentials(self):
        self.assertEqual(self.github["permissions"], {"contents": "read"})
        raw = (ROOT / ".github/workflows/platforms.yml").read_text()
        self.assertNotIn("secrets.", raw)
        self.assertNotIn("continue-on-error", raw)
        self.assertNotIn("--final", raw)

    def test_artifacts_logs_always_retained_and_collision_free(self):
        steps = self.github["jobs"]["build"]["steps"]
        upload = next(x for x in steps if x.get("uses") == "actions/upload-artifact@v4")
        self.assertEqual(upload["if"], "always()")
        self.assertIn("matrix.target", upload["with"]["name"])
        self.assertIn("matrix.profile", upload["with"]["name"])
        self.assertEqual(upload["with"]["if-no-files-found"], "error")
        self.assertEqual(self.gitlab["frontend-build"]["artifacts"]["when"], "always")

    def test_both_providers_call_same_checked_bootstrap_and_build(self):
        steps = self.github["jobs"]["build"]["steps"]
        for command in ["python scripts/ci/frontend.py bootstrap", "python scripts/ci/frontend.py build"]:
            self.assertIn(command, [s.get("run") for s in steps])
            self.assertIn(command, self.gitlab["frontend-build"]["script"])


if __name__ == "__main__":
    unittest.main()

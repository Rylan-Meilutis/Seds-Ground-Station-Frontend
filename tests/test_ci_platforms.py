"""Fast source/behaviour contracts; these do not pretend to compile any platform."""
import contextlib
import gzip
import hashlib
import io
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import tarfile
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

    def test_bootstrap_resolves_lock_and_installs_exact_cargo_and_release_tools(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
             patch.object(ci, "validate_host", return_value=("Linux", "x86_64")), \
             patch.object(ci.shutil, "which", return_value="fixture-tool"), \
             patch.object(ci, "install_binaryen"), \
             patch.object(ci, "install_wasm_bindgen") as wasm:
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
                self.assertIn(["cargo", "+" + ci.RUST, "install", "dioxus-cli", "--version", "=0.7.10"], calls)
                wasm.assert_called_once_with(("Linux", "x86_64"), "0.2.130", unittest.mock.ANY)
                self.assertFalse(any("wasm-bindgen-cli" in argv for argv in calls))
                self.assertFalse(any("--locked" in argv for argv in calls if argv[:3] == ["cargo", "+" + ci.RUST, "install"]))
                receipt = json.loads((Path(directory) / "ci-results/resolved-tools.json").read_text())
                self.assertEqual(receipt["lock_sha256"], ci.file_sha256(Path(directory) / "Cargo.lock"))
                calls.clear()
                ci.bootstrap("web")
                self.assertNotIn(["cargo", "generate-lockfile"], calls)

    def test_wasm_release_pins_cover_all_native_hosts_and_future_abi_fails(self):
        for row in ci.PLATFORMS:
            suffix, digest = ci.wasm_bindgen_asset((row["host"], row["arch"]), "0.2.130")
            self.assertEqual(len(digest), 64)
            self.assertNotIn("latest", suffix)
        with self.assertRaisesRegex(RuntimeError, "update release checksums"):
            ci.wasm_bindgen_asset(("Linux", "x86_64"), "0.2.131")
        with self.assertRaisesRegex(RuntimeError, "No pinned official"):
            ci.wasm_bindgen_asset(("Other", "x86_64"), "0.2.130")

    def wasm_archive(self, extra=None, reported="0.2.130"):
        stem = "wasm-bindgen-0.2.130-x86_64-unknown-linux-musl"
        content = f"#!/bin/sh\nprintf 'wasm-bindgen {reported}\\n'\n".encode()
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w:gz") as archive:
            member = tarfile.TarInfo(stem + "/wasm-bindgen")
            member.size = len(content)
            archive.addfile(member, io.BytesIO(content))
            if extra is not None:
                archive.addfile(extra)
        return output.getvalue(), content

    def install_fixture(self, root, raw, expected=None):
        pins = {"0.2.130": {("Linux", "x86_64"): (
            "x86_64-unknown-linux-musl", expected or hashlib.sha256(raw).hexdigest())}}
        with patch.object(ci, "ROOT", root), patch.object(ci, "WASM_BINDGEN", pins), \
             patch.object(ci.urllib.request, "urlopen", return_value=io.BytesIO(raw)) as download:
            with contextlib.redirect_stdout(io.StringIO()):
                ci.install_wasm_bindgen(("Linux", "x86_64"), "0.2.130", os.environ.copy())
            self.assertEqual(download.call_args.kwargs, {"timeout": 120})
            self.assertTrue(download.call_args.args[0].startswith(
                "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/0.2.130/"))

    @unittest.skipIf(os.name == "nt", "POSIX shell executable fixture; Windows release pins separately checked")
    def test_wasm_release_verified_version_receipt_and_atomic_install(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw, binary = self.wasm_archive()
            self.install_fixture(root, raw)
            dest = root / ".ci-tools/wasm-bindgen/wasm-bindgen"
            self.assertEqual(dest.read_bytes(), binary)
            receipt = json.loads((root / "ci-results/wasm-bindgen-release.json").read_text())
            self.assertEqual(receipt["binary_sha256"], hashlib.sha256(binary).hexdigest())
            self.assertEqual(receipt["archive_sha256"], hashlib.sha256(raw).hexdigest())
            self.assertEqual(receipt["reported"], "wasm-bindgen 0.2.130")
            self.assertEqual(list((root / ".ci-tools").iterdir()), [dest.parent])

    def test_wasm_bad_checksum_never_executes_or_publishes(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "checked") as checked:
            root = Path(directory)
            raw, _ = self.wasm_archive()
            with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                self.install_fixture(root, raw, "0" * 64)
            checked.assert_not_called()
            self.assertFalse((root / ".ci-tools/wasm-bindgen").exists())

    def test_wasm_download_deadline_stops_before_execution(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "checked") as checked, \
             patch.object(ci.time, "monotonic", side_effect=[0, 121]):
            raw, _ = self.wasm_archive()
            with self.assertRaisesRegex(RuntimeError, "120 seconds"):
                self.install_fixture(Path(directory), raw)
            checked.assert_not_called()

    def test_wasm_malicious_members_never_execute_or_write_foreign_paths(self):
        stem = "wasm-bindgen-0.2.130-x86_64-unknown-linux-musl"
        for name, kind in [("/escape", tarfile.REGTYPE), (stem + "/../escape", tarfile.REGTYPE),
                           (stem + "/C:\\escape", tarfile.REGTYPE),
                           (stem + "/link", tarfile.SYMTYPE), (stem + "/hard", tarfile.LNKTYPE),
                           (stem + "/fifo", tarfile.FIFOTYPE),
                           (stem + "/wasm-bindgen", tarfile.REGTYPE)]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory, \
                 patch.object(ci, "checked") as checked:
                root = Path(directory)
                member = tarfile.TarInfo(name)
                member.type = kind
                member.linkname = "../../foreign"
                raw, _ = self.wasm_archive(member)
                with self.assertRaisesRegex(RuntimeError, "Unsafe|Duplicate"):
                    self.install_fixture(root, raw)
                checked.assert_not_called()
                self.assertEqual(list((root / ".ci-tools").iterdir()), [])
                self.assertFalse((root / "escape").exists())

    @unittest.skipIf(os.name == "nt", "POSIX shell executable fixture; Windows release pins separately checked")
    def test_wasm_wrong_version_and_warning_are_fatal_without_publishing(self):
        for reported in ["0.2.129", "0.2.130\\nwarning: causal version fixture"]:
            with self.subTest(reported=reported), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                raw, _ = self.wasm_archive(reported=reported)
                with self.assertRaisesRegex(RuntimeError, "version mismatch|warning_lines=1"):
                    self.install_fixture(root, raw)
                self.assertFalse((root / ".ci-tools/wasm-bindgen").exists())
                self.assertTrue((root / "ci-results/wasm-bindgen-version.log").is_file())

    def test_wasm_refuses_stale_directory_without_deleting_contents(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dest = root / ".ci-tools/wasm-bindgen"
            dest.mkdir(parents=True)
            (dest / "owned").write_text("keep")
            raw, _ = self.wasm_archive()
            with self.assertRaisesRegex(RuntimeError, "stale"):
                self.install_fixture(root, raw)
            self.assertEqual((dest / "owned").read_text(), "keep")

    def test_wasm_missing_executable_and_oversized_members_never_execute(self):
        stem = "wasm-bindgen-0.2.130-x86_64-unknown-linux-musl"
        for size in [0, 257 * 1024 * 1024]:
            with self.subTest(size=size), tempfile.TemporaryDirectory() as directory, \
                 patch.object(ci, "checked") as checked:
                member = tarfile.TarInfo(stem + "/README.md")
                member.size = size
                # Deliberately hostile declared size without allocating its body.
                raw = gzip.compress(member.tobuf() + b"\0" * 1024)
                with self.assertRaisesRegex(RuntimeError, "Missing|exceeds 256"):
                    self.install_fixture(Path(directory), raw)
                checked.assert_not_called()

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

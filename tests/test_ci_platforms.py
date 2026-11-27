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
        self.assertEqual(len(ci.PLATFORMS), 17)

    def test_github_both_profiles_and_no_fail_fast_hiding_platforms(self):
        strategy = self.github["jobs"]["build"]["strategy"]
        self.assertEqual(strategy["matrix"]["target"], list(targets.TARGETS))
        self.assertEqual(strategy["matrix"]["profile"], ["debug", "release"])
        self.assertIs(strategy["fail-fast"], False)
        # Release for every target, debug only where platforms.json says so.
        debug = [p["target"] for p in ci.PLATFORMS if p["debug"]]
        self.assertEqual(debug, ["web", "linux", "android_sim", "wearos_sim"])
        self.assertEqual(strategy["matrix"]["exclude"],
                         [{"target": p["target"], "profile": "debug"} for p in ci.PLATFORMS if not p["debug"]])
        self.assertEqual(len(strategy["matrix"]["target"]) * 2 - len(strategy["matrix"]["exclude"]), 21)
        # The launch test installs these debug builds.
        launch = self.github["jobs"]["android-launch"]["strategy"]["matrix"]["include"]
        self.assertTrue({r["target"] for r in launch if r["profile"] == "debug"} <= set(debug))

    def test_github_native_runner_bindings(self):
        matrix = self.github["jobs"]["build"]["strategy"]["matrix"]
        self.assertEqual(matrix["include"], [{"target": p["target"], "runner": p["github"], "deps_cache": p["deps_cache"]}
                                             for p in ci.PLATFORMS])
        self.assertEqual(self.github["jobs"]["build"]["runs-on"], "${{ matrix.runner }}")

    def test_apple_silicon_targets_build_with_xcode_27(self):
        # ITMS-90725: App Store Connect requires the iOS 27 SDK. The xcode-27
        # image is Apple silicon only; Intel targets stay on Xcode 26.
        rows = {p["target"]: p for p in ci.PLATFORMS}
        for p in ci.PLATFORMS:
            if p["host"] == "Darwin":
                expected = "xcode-27" if p["arch"] == "aarch64" else "macos-15-intel"
                self.assertEqual(p["github"], expected, p["target"])
        self.assertEqual(sorted(t for t, p in rows.items() if p["github"] == "xcode-27"),
                         ["ios", "ios_sim", "macos", "watchos", "watchos_sim"])
        steps = self.github["jobs"]["build"]["steps"]
        xcode = next(s for s in steps if s.get("name") == "Xcode 27 (iOS 27 SDK) on Apple silicon")
        self.assertEqual(xcode["if"], "matrix.runner == 'xcode-27'")
        self.assertIn("DEVELOPER_DIR=/Applications/Xcode_27.2.app/Contents/Developer", xcode["run"])
        intel = next(s for s in steps if s.get("name") == "Xcode 26 for the Intel iOS simulator")
        self.assertIn("matrix.runner != 'xcode-27'", intel["if"])

    def test_android_ndk_links_16kb_pages_by_default(self):
        # NDK r28+ aligns LOAD segments to 16 KB by default (Play requirement).
        ndk = "28.2.13676358"
        steps = self.github["jobs"]["build"]["steps"]
        sdk = next(s for s in steps if s.get("name") == "Android SDK and NDK")
        self.assertIn(f'"ndk;{ndk}"', sdk["run"])
        self.assertIn(f"ANDROID_NDK_HOME=$ANDROID_HOME/ndk/{ndk}", sdk["run"])
        # The hosted image presets ANDROID_NDK_ROOT to its own r27 default.
        self.assertIn(f"ANDROID_NDK_ROOT=$ANDROID_HOME/ndk/{ndk}", sdk["run"])
        for path in ["scripts/ci/gitlab_docker_prereqs.sh", "Dockerfile.builder", ".github/workflows/release.yml"]:
            text = (ROOT / path).read_text()
            self.assertIn(ndk, text, path)
            self.assertNotRegex(text, r"ndk;2[0-7]\.|ndk/2[0-7]\.|GS_NDK=2[0-7]\.", path)

    def test_android_launch_smoke_on_emulators(self):
        job = self.github["jobs"]["android-launch"]
        self.assertEqual((job["needs"], job["if"], job["runs-on"]), ("build", "${{ !cancelled() }}", "ubuntu-24.04"))
        rows = [(r["target"], r["profile"], r["api"]) for r in job["strategy"]["matrix"]["include"]]
        self.assertEqual(rows, [("android_sim", "debug", 36), ("android_sim", "release", 36),
                                ("android_sim", "release", 24), ("wearos_sim", "debug", 34)])
        # The release Wear APK is unsigned here and will not install; API 24's
        # google_apis image hung in boot.
        images = {(r["target"], r["api"]): r["image"] for r in job["strategy"]["matrix"]["include"]}
        self.assertEqual(images[("android_sim", 24)], "default")
        # The API 24 image hangs in boot with 4 GB of RAM.
        rams = {(r["target"], r["api"]): r["ram"] for r in job["strategy"]["matrix"]["include"]}
        self.assertEqual(rams, {("android_sim", 36): "4096M", ("android_sim", 24): "2048M", ("wearos_sim", 34): "4096M"})
        for row in job["strategy"]["matrix"]["include"]:
            # Only targets the build job makes, as x86_64 for the emulator.
            self.assertIn(row["target"], [p["target"] for p in ci.PLATFORMS])
            expected = ("" if row["target"] == "wearos_sim" else
                        "--require-launch-log --no-screen-check" if row["api"] == 24 else "--require-render-log")
            self.assertEqual(row["render"], expected)
        download = next(s for s in job["steps"] if s.get("uses") == "actions/download-artifact@v7")
        self.assertEqual(download["with"]["name"], "frontend-${{ matrix.target }}-${{ matrix.profile }}")
        emulator = next(s for s in job["steps"] if s.get("uses", "").startswith("reactivecircus/android-emulator-runner@"))
        self.assertRegex(emulator["uses"], r"@[0-9a-f]{40}$")
        self.assertLessEqual(emulator["timeout-minutes"], job["timeout-minutes"] - 10, "a hung emulator fails the step")
        self.assertEqual(emulator["with"]["arch"], "x86_64")
        self.assertEqual(emulator["with"]["ram-size"], "${{ matrix.ram }}")
        self.assertIn("scripts/android_launch_smoke.py", emulator["with"]["script"])
        kvm = next(s for s in job["steps"] if s.get("name") == "Enable KVM")
        self.assertIn("udevadm trigger --name-match=kvm", kvm["run"])
        upload = job["steps"][-1]
        self.assertEqual(upload["if"], "always()")
        self.assertIn("launch-output", upload["with"]["path"])
        # Emulator output never passes through the build step's warning gate.
        self.assertNotIn("frontend.py", json.dumps(job))

    def gitlab_rows(self):
        return (self.gitlab["frontend-build-docker"]["parallel"]["matrix"]
                + self.gitlab["frontend-build"]["parallel"]["matrix"])

    def test_gitlab_has_all24_real_native_jobs_no_allow_failure(self):
        rows = self.gitlab_rows()
        self.assertEqual(sorted(r["CI_FRONTEND_TARGET"] for r in rows), sorted(targets.TARGETS))
        self.assertEqual(len(rows), len(targets.TARGETS))
        debug = {p["target"]: p["debug"] for p in ci.PLATFORMS}
        for r in rows:
            self.assertEqual(r["CI_FRONTEND_PROFILE"], ["debug", "release"] if debug[r["CI_FRONTEND_TARGET"]] else ["release"])
        native, docker = self.gitlab["frontend-build"], self.gitlab["frontend-build-docker"]
        self.assertEqual(native["tags"], ["$UI_RUNNER"])
        for job in [native, docker, self.gitlab[".frontend-build"]]:
            self.assertNotIn("allow_failure", job)
        bindings = {p["target"]: p["gitlab"] for p in ci.PLATFORMS}
        self.assertEqual({r["CI_FRONTEND_TARGET"]: r["UI_RUNNER"] for r in native["parallel"]["matrix"]},
                         {t: g for t, g in bindings.items() if g != "docker"})
        self.assertEqual([r["CI_FRONTEND_TARGET"] for r in docker["parallel"]["matrix"]],
                         [t for t, g in bindings.items() if g == "docker"])

    def test_gitlab_native_jobs_are_opt_in_and_jobs_are_interruptible(self):
        # Without registered native runners those jobs stay pending, and a
        # pipeline holding them exceeds the project's active-job limit, so
        # GitLab refuses to create it. They run only when enabled.
        native, docker = self.gitlab["frontend-build"], self.gitlab["frontend-build-docker"]
        self.assertEqual(native["rules"], [{"if": '$GS_UI_NATIVE_RUNNERS == "true"'}])
        self.assertNotIn("rules", docker)
        self.assertTrue(self.gitlab["ci-contracts"]["interruptible"])
        self.assertTrue(self.gitlab[".frontend-build"]["interruptible"])
        default_jobs = 1 + sum(len(r["CI_FRONTEND_PROFILE"]) for r in docker["parallel"]["matrix"])
        self.assertEqual(default_jobs, 10)

    def test_gitlab_generic_docker_runners_only_for_container_safe_linux_targets(self):
        docker = self.gitlab["frontend-build-docker"]
        self.assertNotIn("tags", docker)
        self.assertEqual(docker["image"], "python:3.11-bookworm")
        self.assertEqual(docker["before_script"], [". scripts/ci/gitlab_docker_prereqs.sh"])
        self.assertTrue(all("UI_RUNNER" not in r for r in docker["parallel"]["matrix"]))
        names = {r["CI_FRONTEND_TARGET"] for r in docker["parallel"]["matrix"]}
        self.assertEqual(names, {"web", "android", "android-armv7", "android_sim", "wearos", "wearos_sim"})
        for p in ci.PLATFORMS:
            if p["target"] in names:
                self.assertEqual((p["host"], p["arch"]), ("Linux", "x86_64"))
        # Desktop Linux packaging (Flatpak, AppImage, Ubuntu makepkg) stays native.
        self.assertEqual({p["target"]: p["gitlab"] for p in ci.PLATFORMS}["linux"], "gs-ui-linux-x64")
        self.assertNotIn("tags", self.gitlab["ci-contracts"])
        prereqs = (ROOT / "scripts/ci/gitlab_docker_prereqs.sh").read_text()
        self.assertEqual(prereqs.count("sha256sum --check"), 2)
        self.assertNotIn("secret", prereqs.lower().replace("reads a secret", ""))
        raw = (ROOT / ".gitlab-ci.yml").read_text()
        for forbidden in ["--final", "release-", "CI_COMMIT_TAG", "secrets"]:
            self.assertNotIn(forbidden, raw)

    def test_apple_windows_linux_wrong_host_is_error_not_skip(self):
        for name in ["ios", "ios_sim", "ios_sim_intel", "watchos", "watchos_sim", "watchos_sim_intel", "macos", "macos-intel", "windows", "linux-arm64"]:
            with self.subTest(target=name), patch.object(ci.platform, "system", return_value="Linux"), patch.object(ci.platform, "machine", return_value="x86_64"):
                with self.assertRaises(RuntimeError):
                    ci.validate_host(name)

    def test_all_declared_native_hosts_accepted(self):
        for p in ci.PLATFORMS:
            with self.subTest(target=p["target"]), patch.object(ci.platform, "system", return_value=p["host"]), patch.object(ci.platform, "machine", return_value=p["arch"]):
                self.assertEqual(ci.validate_host(p["target"]), (p["host"], p["arch"]))

    def test_watch_targets_bind_to_macos_or_linux_runners(self):
        rows = {p["target"]: p for p in ci.PLATFORMS}
        for name, arch in [("watchos", "aarch64"), ("watchos_sim", "aarch64"), ("watchos_sim_intel", "x86_64")]:
            self.assertEqual((rows[name]["host"], rows[name]["arch"]), ("Darwin", arch))
            self.assertTrue(rows[name]["gitlab"].startswith("gs-ui-macos-"))
            self.assertTrue(rows[name]["github"].startswith(("macos-", "xcode-")))
        for name in ["wearos", "wearos_sim"]:
            self.assertEqual(rows[name]["host"], "Linux")
            self.assertTrue(ci.is_wear(name) and ci.android_signed(name))
        self.assertFalse(ci.is_wear("android") or ci.is_wear("watchos"))
        steps = self.github["jobs"]["build"]["steps"]
        sdk = next(s for s in steps if s.get("name") == "Android SDK and NDK")
        self.assertIn("wearos", sdk["if"])
        self.assertIn("platforms;android-36", sdk["run"])
        watch = next(s for s in steps if s.get("name") == "watchOS SDK and XcodeGen")
        self.assertIn("xcodegen", watch["run"])

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
                self.assertIn(["cargo", "+" + ci.RUST, "install", "dioxus-cli", "--version", "=0.7.10",
                               "--root", str(Path(directory) / ".ci-tools/dx")], calls)
                wasm.assert_called_once_with(("Linux", "x86_64"), "0.2.130", unittest.mock.ANY)
                self.assertFalse(any("wasm-bindgen-cli" in argv for argv in calls))
                self.assertFalse(any("--locked" in argv for argv in calls if argv[:3] == ["cargo", "+" + ci.RUST, "install"]))
                receipt = json.loads((Path(directory) / "ci-results/resolved-tools.json").read_text())
                self.assertEqual(receipt["lock_sha256"], ci.file_sha256(Path(directory) / "Cargo.lock"))
                calls.clear()
                ci.bootstrap("web")
                self.assertNotIn(["cargo", "generate-lockfile"], calls)

    def test_cached_dx_is_used_only_at_the_resolved_version(self):
        def fake_dx(directory, reported):
            dx = Path(directory) / ".ci-tools/dx/bin/dx"
            dx.parent.mkdir(parents=True)
            dx.write_text(f"#!/bin/sh\nprintf '{reported}\\n'\n")
            dx.chmod(0o755)
        for reported, reused in [("dioxus 0.7.10 (57d6794)", True), ("dioxus 0.7.9 (1234567)", False),
                                 ("dioxus 0.7.100 (1234567)", False), ("", False)]:
            with self.subTest(reported=reported), tempfile.TemporaryDirectory() as directory, \
                 patch.object(ci, "ROOT", Path(directory)), patch.object(ci, "checked") as checked:
                (Path(directory) / "ci-results").mkdir()
                output = Path(directory) / "github-output"
                fake_dx(directory, reported)
                with patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}):
                    ci.install_dioxus_cli(("Linux", "x86_64"), "0.7.10", os.environ.copy())
                self.assertEqual(checked.called, not reused)
                if not reused:
                    self.assertFalse((Path(directory) / ".ci-tools/dx/bin/dx").exists(), "a stale dx is removed first")
                self.assertIn(f"dx-built={'false' if reused else 'true'}", output.read_text())
                self.assertIn(f"dx-version={ci.RUST}-0.7.10", output.read_text())
                receipt = json.loads((Path(directory) / "ci-results/dioxus-cli.json").read_text())
                self.assertEqual(receipt["reused_from_cache"], reused)

    def test_dependency_cache_after_bootstrap_shared_with_release(self):
        job = self.github["jobs"]["build"]
        names = [s.get("name") for s in job["steps"]]
        step = job["steps"][names.index("Cached compiled dependencies")]
        self.assertRegex(step["uses"], r"^Swatinem/rust-cache@[0-9a-f]{40}$")
        self.assertLess(names.index("Pinned frontend tools, warnings fatal"), names.index("Cached compiled dependencies"))
        self.assertLess(names.index("Cached compiled dependencies"), names.index("Actual native target build, warnings fatal"))
        # The cache restores Cargo's registry index as saved, which can predate the
        # lock bootstrap resolved (cc 1.12.0 in hosted run 45950452142): fetch again.
        refetch = job["steps"][names.index("Cached compiled dependencies") + 1]
        self.assertEqual((refetch["run"], refetch["if"]), ("python scripts/ci/frontend.py refetch", step["if"]))
        self.assertEqual(step["with"]["shared-key"], "${{ matrix.target }}-${{ matrix.profile }}")
        # Only where it measurably paid off, release rows only, saved by dev only: the
        # repository keeps 10 GB of caches and evicts the least recently used.
        self.assertEqual(step["if"], "matrix.deps_cache && matrix.profile == 'release'")
        self.assertEqual(step["with"]["save-if"], "${{ github.event_name == 'push' && github.ref == 'refs/heads/dev' }}")
        self.assertEqual([p["target"] for p in ci.PLATFORMS if p["deps_cache"]],
                         ["linux", "linux-arm64", "macos", "macos-intel", "android", "ios_sim_intel", "android_sim",
                          "watchos_sim_intel"])
        self.assertIs(step["with"]["add-job-id-key"], False)
        release = yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())
        count = 0
        for name, rjob in release["jobs"].items():
            steps = rjob.get("steps", [])
            for i, s in enumerate(steps):
                if s.get("run") == "python scripts/ci/frontend.py bootstrap":
                    count += 1
                    after = steps[i + 1]
                    self.assertEqual(after["uses"], step["uses"], name)
                    self.assertEqual({k: after["with"][k] for k in ("prefix-key", "add-job-id-key", "save-if")},
                                     {"prefix-key": step["with"]["prefix-key"], "add-job-id-key": False, "save-if": False}, name)
                    self.assertEqual(after["with"]["shared-key"], "${{ env.CI_FRONTEND_TARGET }}-release", name)
                    self.assertEqual(after.get("if"), s.get("if"), name)
                    refetch = steps[i + 2]
                    self.assertEqual((refetch["run"], refetch.get("if")), ("python scripts/ci/frontend.py refetch", s.get("if")), name)
        self.assertEqual(count, 6)
        # The key hashes CARGO_*/RUST* step env; both workflows must agree.
        self.assertEqual({k: v for k, v in job["env"].items() if k.startswith(("CARGO_", "RUST"))},
                         {k: v for k, v in release["env"].items() if k.startswith(("CARGO_", "RUST"))})

    def test_refetch_fetches_the_current_lock_with_its_own_receipt(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
             patch.object(ci, "checked") as checked, patch.object(sys, "argv", ["frontend.py", "refetch"]):
            ci.main()
        argv, _env, label = checked.call_args.args
        self.assertEqual((argv, label), (["cargo", "fetch", "--locked"], "application-refetch"))

    def test_dx_cache_is_build_only_ci_keyed_by_host_and_target(self):
        job = self.github["jobs"]["build"]
        names = [s.get("name") for s in job["steps"]]
        restore = job["steps"][names.index("Cached dx build tool")]
        save = job["steps"][names.index("Save the dx build tool")]
        bootstrap = names.index("Pinned frontend tools, warnings fatal")
        self.assertLess(names.index("Cached dx build tool"), bootstrap)
        self.assertLess(bootstrap, names.index("Save the dx build tool"))
        self.assertEqual(job["steps"][bootstrap]["id"], "bootstrap")
        prefix = "dx-v1-${{ runner.os }}-${{ runner.arch }}-${{ matrix.target }}--"
        self.assertEqual((restore["with"]["path"], restore["with"]["restore-keys"]), (".ci-tools/dx", prefix))
        self.assertEqual(save["with"]["key"], prefix + "${{ steps.bootstrap.outputs.dx-version }}")
        self.assertEqual(save["if"], "github.event_name == 'push' && matrix.profile == 'release' && steps.bootstrap.outputs.dx-built == 'true'")
        # Prefix matching must not cross targets (android vs android-armv7).
        targets = [r["target"] for r in ci.PLATFORMS]
        for a in targets:
            for b in targets:
                if a != b:
                    self.assertFalse(f"{b}--".startswith(f"{a}--"))
        # The release workflow restores the same keys before every bootstrap, never saves.
        release = yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())
        release_prefix = "dx-v1-${{ runner.os }}-${{ runner.arch }}-${{ env.CI_FRONTEND_TARGET }}--"
        bootstraps = 0
        for name, rjob in release["jobs"].items():
            steps = rjob.get("steps", [])
            for i, step in enumerate(steps):
                if step.get("run") == "python scripts/ci/frontend.py bootstrap":
                    bootstraps += 1
                    before = steps[i - 1]
                    self.assertEqual((before["uses"], before["with"]["path"], before["with"]["restore-keys"]),
                                     ("actions/cache/restore@v5", ".ci-tools/dx", release_prefix), name)
                    self.assertEqual(before.get("if"), step.get("if"), name)
                    self.assertIn(rjob["env"]["CI_FRONTEND_TARGET"].replace("${{ matrix.target }}", targets[0]),
                                  targets, name)
                self.assertFalse(step.get("uses", "").startswith("actions/cache/save"), name)
        self.assertEqual(bootstraps, 6)

    def test_actions_run_on_node_24_and_msvc_comes_from_vcvars(self):
        # Node 20 actions are deprecated on the hosted runners; ilammy/msvc-dev-cmd has no Node 24 release.
        for name in ("platforms.yml", "release.yml"):
            text = (ROOT / ".github/workflows" / name).read_text()
            self.assertNotIn("uses: ilammy/", text)
            for old in ("checkout@v4", "setup-python@v5", "setup-node@v4", "setup-java@v4", "cache/restore@v4",
                        "cache/save@v4", "upload-artifact@v4", "download-artifact@v4", "setup-android@v3"):
                self.assertNotIn(old, text, name)
            workflow = yaml.safe_load(text)
            msvc = [s for job in workflow["jobs"].values() for s in job.get("steps", []) if s.get("name") == "Windows MSVC environment"]
            self.assertEqual(len(msvc), 1, name)
            self.assertEqual((msvc[0]["if"], msvc[0]["shell"]), ("runner.os == 'Windows'", "pwsh"))
            for needle in ("vswhere.exe", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "vcvars64.bat", "GITHUB_ENV", "$LASTEXITCODE"):
                self.assertIn(needle, msvc[0]["run"], name)

    def test_hosted_runners_use_every_core_gitlab_keeps_two(self):
        self.assertEqual(self.github["jobs"]["build"]["env"]["CI_CARGO_JOBS"], "cores")
        self.assertEqual(yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())["env"]["CI_CARGO_JOBS"], "cores")
        with patch.dict(os.environ, {"CI_CARGO_JOBS": "cores"}):
            self.assertEqual(ci.ci_env()["CARGO_BUILD_JOBS"], str(os.cpu_count()))
        self.assertNotIn("CI_CARGO_JOBS", (ROOT / ".gitlab-ci.yml").read_text())

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
            # The default; release.yml's own CI_CARGO_JOBS=cores is set while this runs there.
            os.environ.pop("CI_CARGO_JOBS", None)
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
        upload = next(x for x in steps if x.get("uses") == "actions/upload-artifact@v6")
        self.assertEqual(upload["if"], "always()")
        self.assertIn("matrix.target", upload["with"]["name"])
        self.assertIn("matrix.profile", upload["with"]["name"])
        self.assertEqual(upload["with"]["if-no-files-found"], "error")
        self.assertEqual(self.gitlab[".frontend-build"]["artifacts"]["when"], "always")

    def test_both_providers_call_same_checked_bootstrap_and_build(self):
        steps = self.github["jobs"]["build"]["steps"]
        for command in ["python scripts/ci/frontend.py bootstrap", "python scripts/ci/frontend.py build"]:
            self.assertIn(command, [s.get("run") for s in steps])
            self.assertIn(command, self.gitlab[".frontend-build"]["script"])
        for job in ["frontend-build", "frontend-build-docker"]:
            self.assertEqual(self.gitlab[job]["extends"], ".frontend-build")
            self.assertNotIn("script", self.gitlab[job])


if __name__ == "__main__":
    unittest.main()

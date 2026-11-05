"""Actual regression controls for the b237 hosted CI failures; no platform build claim."""
import contextlib
import hashlib
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
import urllib.error

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("ci_failure_driver", ROOT / "scripts/ci/frontend.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


class HostedFailures(unittest.TestCase):
    def test_real_cp1252_console_preserves_utf8_diagnostics_and_fatal_warning(self):
        script = r"""
import importlib.util,json,os,pathlib,sys
spec=importlib.util.spec_from_file_location('driver',sys.argv[1]);ci=importlib.util.module_from_spec(spec);spec.loader.exec_module(ci)
ci.ROOT=pathlib.Path(sys.argv[2])
assert sys.stdout.encoding.lower()=='cp1252'
assert sys.stderr.encoding.lower()=='cp1252'
ci.checked([sys.executable,'-c',"print('\\ufffd \\u2713',flush=True); import sys; print('stderr \\u03a9',file=sys.stderr)"],ci.ci_env(),'unicode')
try:ci.checked([sys.executable,'-c',"print('warning: diagnostic \\ufffd')"],ci.ci_env(),'warning')
except RuntimeError:pass
else:raise AssertionError('Warning must remain fatal')
assert sys.stdout.encoding.lower()=='utf-8' and sys.stderr.encoding.lower()=='utf-8'
"""
        with tempfile.TemporaryDirectory() as directory:
            env = os.environ.copy()
            env.update(PYTHONUTF8="0", PYTHONIOENCODING="cp1252:strict")
            result = subprocess.run([sys.executable, "-c", script, str(ROOT / "scripts/ci/frontend.py"), directory],
                                    env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", errors="replace"))
            self.assertIn("\ufffd \u2713".encode("utf-8"), result.stdout)
            self.assertIn("stderr \u03a9".encode("utf-8"), result.stdout)
            self.assertEqual((Path(directory) / "ci-results/unicode.log").read_text(), "\ufffd \u2713\nstderr \u03a9\n")
            self.assertEqual(json.loads((Path(directory) / "ci-results/warning.json").read_text())["warning_lines"], 1)

    def test_utf8_configuration_respects_nonreconfigurable_test_streams(self):
        out, err = io.StringIO(), io.StringIO()
        with patch.object(ci.sys, "stdout", out), patch.object(ci.sys, "stderr", err):
            ci.configure_utf8_console()
            print("\ufffd \u2713")
        self.assertEqual(out.getvalue(), "\ufffd \u2713\n")

    def test_child_utf8_environment_preserves_other_inherited_values(self):
        with patch.dict(os.environ, {"CI_FIXTURE_MARKER": "retained", "PYTHONUTF8": "0", "PYTHONIOENCODING": "cp1252"}):
            env = ci.ci_env()
        self.assertEqual(env["CI_FIXTURE_MARKER"], "retained")
        self.assertEqual(env["PYTHONUTF8"], "1")
        self.assertEqual(env["PYTHONIOENCODING"], "utf-8")

    def test_actual_subprocess_uses_absolute_tool_resolved_from_child_path(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)):
            root = Path(directory)
            tool = root / "custom-path/python-host-tool"
            tool.parent.mkdir()
            if os.name == "nt":
                self.skipTest("POSIX executable fixture; Windows path-resolution argv contract is shared")
            tool.write_text("#!/bin/sh\nprintf 'prepared tool\\n'\n")
            tool.chmod(0o755)
            with contextlib.redirect_stdout(io.StringIO()):
                ci.checked([tool.name], {"PATH": str(tool.parent)}, "tool")
            receipt = json.loads((root / "ci-results/tool.json").read_text())
            self.assertEqual(receipt["resolved_argv"], [str(tool)])
            self.assertEqual(receipt["exit"], 0)

    def test_verified_download_retries_504_and_partial_transport_then_succeeds(self):
        class Partial(io.BytesIO):
            def read(self, size=-1):
                raise urllib.error.URLError("fixture disconnected")
        with tempfile.TemporaryDirectory() as directory, patch.object(ci.time, "sleep") as delay, \
             patch.object(ci.urllib.request, "urlopen", side_effect=[
                 urllib.error.HTTPError("https://fixture", 504, "fixture", {}, None),
                 Partial(b"partial"), io.BytesIO(b"verified bytes")]) as request:
            archive = Path(directory) / "owned-download"
            ci.download("https://fixture", archive)
            self.assertEqual(archive.read_bytes(), b"verified bytes")
            self.assertEqual(request.call_count, 3)
            self.assertEqual([call.args[0] for call in delay.call_args_list], [1, 2])

    def test_nontransient_and_exhausted_downloads_are_errors_not_skips(self):
        for code, expected in [(404, 1), (504, 3)]:
            with self.subTest(code=code), tempfile.TemporaryDirectory() as directory, \
                 patch.object(ci.time, "sleep"), patch.object(ci.urllib.request, "urlopen",
                 side_effect=urllib.error.HTTPError("https://fixture", code, "fixture", {}, None)) as request:
                archive = Path(directory) / "owned-download"
                with self.assertRaises(urllib.error.HTTPError):
                    ci.download("https://fixture", archive)
                self.assertEqual(request.call_count, expected)
                self.assertFalse(archive.exists())

    def test_download_size_bound_is_not_retried_or_executed(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci.time, "sleep") as delay, \
             patch.object(ci.urllib.request, "urlopen", return_value=io.BytesIO(b"oversize")) as request:
            with self.assertRaisesRegex(RuntimeError, "size bound"):
                ci.download("https://fixture", Path(directory) / "download", limit=1)
            self.assertEqual(request.call_count, 1)
            delay.assert_not_called()

    def test_exact_upstream_progress_is_preserved_and_reported_only_for_build(self):
        line = "warning: Waiting for cargo-metadata..."
        for enabled in [False, True]:
            with self.subTest(enabled=enabled), tempfile.TemporaryDirectory() as directory, \
                 patch.object(ci, "ROOT", Path(directory)), contextlib.redirect_stdout(io.StringIO()) as console:
                call = lambda: ci.checked([sys.executable, "-c", f"print({line!r})"], os.environ.copy(), "build", dioxus_progress=enabled)
                if enabled:
                    call()
                else:
                    with self.assertRaisesRegex(RuntimeError, "warning_lines=1"):
                        call()
                self.assertEqual(console.getvalue(), line + "\n")
                receipt = json.loads((Path(directory) / "ci-results/build.json").read_text())
                self.assertEqual(receipt["external_metadata_progress_warnings"], [line] if enabled else [])
                self.assertEqual(receipt["warning_lines"], 0 if enabled else 1)

    def test_real_compiler_and_unrecognized_tool_warnings_still_fail_after_progress(self):
        for warning in ["warning: unused item", "warning[dead_code]: item", "WARN resource missing",
                        "warning: Waiting for cargo-metadata... unexpected", "warning: (Try 101) Taking a while..."]:
            with self.subTest(warning=warning), tempfile.TemporaryDirectory() as directory, \
                 patch.object(ci, "ROOT", Path(directory)), contextlib.redirect_stdout(io.StringIO()):
                text = "warning: (Try 1) Taking a while...\n" + warning
                with self.assertRaisesRegex(RuntimeError, "warning_lines=1"):
                    ci.checked([sys.executable, "-c", f"print({text!r})"], os.environ.copy(), "build", dioxus_progress=True)
                receipt = json.loads((Path(directory) / "ci-results/build.json").read_text())
                self.assertEqual(receipt["external_metadata_progress_warnings"], ["warning: (Try 1) Taking a while..."])
                self.assertEqual(receipt["warning_lines"], 1)

    @staticmethod
    def binaryen_archive():
        import tarfile
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w:gz") as bundle:
            member = tarfile.TarInfo("binaryen-version_133/bin/wasm-opt")
            body = b"verified binary fixture"
            member.size = len(body)
            member.mode = 0o755
            bundle.addfile(member, io.BytesIO(body))
        return output.getvalue()

    def test_binaryen_bound_covers_exact_pinned_official_sizes_without_changing_other_caps(self):
        official_sizes = {
            ("Linux", "x86_64"): 111200018,
            ("Linux", "aarch64"): 105577739,
            ("Darwin", "aarch64"): 7810532,
            ("Darwin", "x86_64"): 9276271,
            ("Windows", "x86_64"): 111045528,
        }
        self.assertEqual(ci.download.__kwdefaults__["limit"], 64 * 1024 * 1024)
        for host, official_bytes in official_sizes.items():
            with self.subTest(host=host), tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)):
                root = Path(directory)
                raw = self.binaryen_archive()
                suffix, _ = ci.BINARYEN[host]
                def prepared_download(_url, archive, *, limit):
                    self.assertEqual(limit, 128 * 1024 * 1024)
                    self.assertGreaterEqual(limit, official_bytes)
                    archive.write_bytes(raw)
                with patch.dict(ci.BINARYEN, {host: (suffix, hashlib.sha256(raw).hexdigest())}), patch.object(ci, "download", side_effect=prepared_download) as fetch:
                    ci.install_binaryen(host)
                fetch.assert_called_once()
                self.assertEqual((root / ".ci-tools/binaryen/bin/wasm-opt").read_bytes(), b"verified binary fixture")

    def test_binaryen_larger_cap_still_refuses_checksum_before_extract_or_execution(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), patch.object(ci, "checked") as execute:
            root = Path(directory)
            with patch.object(ci.urllib.request, "urlopen", return_value=io.BytesIO(self.binaryen_archive())):
                with self.assertRaisesRegex(RuntimeError, "Binaryen archive checksum mismatch"):
                    ci.install_binaryen(("Linux", "x86_64"))
            self.assertFalse((root / ".ci-tools/binaryen").exists())
            execute.assert_not_called()

    def test_linuxdeploy_mismatch_never_publishes_or_executes(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
             patch.object(ci.urllib.request, "urlopen", return_value=io.BytesIO(b"wrong binary")):
            (Path(directory) / ".ci-tools").mkdir()
            with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                ci.install_linuxdeploy(("Linux", "x86_64"))
            self.assertFalse((Path(directory) / ".ci-tools/linuxdeploy").exists())

    def test_linuxdeploy_verified_native_pin_is_installed(self):
        binary = b"fixture binary"
        for arch in ["x86_64", "aarch64"]:
            with self.subTest(arch=arch), tempfile.TemporaryDirectory() as directory, \
                 patch.object(ci, "ROOT", Path(directory)), \
                 patch.object(ci, "LINUXDEPLOY", {arch: ("fixture", hashlib.sha256(binary).hexdigest())}), \
                 patch.object(ci.urllib.request, "urlopen", return_value=io.BytesIO(binary)):
                root = Path(directory)
                (root / ".ci-tools").mkdir()
                (root / "ci-results").mkdir()
                ci.install_linuxdeploy(("Linux", arch))
                self.assertEqual((root / ".ci-tools/linuxdeploy/bin/linuxdeploy").read_bytes(), binary)
                self.assertEqual(json.loads((root / "ci-results/linuxdeploy-release.json").read_text())["host"], ["Linux", arch])


class PlatformPrerequisites(unittest.TestCase):
    def test_android_setup_excludes_removed_tools_and_both_ci_run_regressions(self):
        import yaml
        github = yaml.safe_load((ROOT / ".github/workflows/platforms.yml").read_text())
        steps = github["jobs"]["build"]["steps"]
        android = next(step for step in steps if step.get("uses") == "android-actions/setup-android@v3")
        self.assertEqual(android["with"]["packages"], "platform-tools")
        linux = next(step for step in steps if step.get("name") == "Linux native libraries")["run"]
        for package in ["makepkg", "pacman-package-manager", "fakeroot", "flatpak", "rpm", "desktop-file-utils", "appstream", "gnupg", "zstd"]:
            self.assertIn(package, linux.split())
        self.assertIn("fuse3", linux.split())
        self.assertIn("libfuse2t64", linux.split())
        self.assertNotIn("fuse", linux.split(), "legacy fuse conflicts with Flatpak's fuse3 dependency")
        for config in [".github/workflows/platforms.yml", ".gitlab-ci.yml"]:
            self.assertIn("-p 'test_ci*.py'", (ROOT / config).read_text())

    def test_actual_github_warning_annotations_are_fatal_and_preserved(self):
        for message in ["::warning::AppStream metadata missing", "::warning file=build.py,line=1::fixture"]:
            with self.subTest(message=message), tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), contextlib.redirect_stdout(io.StringIO()) as output:
                with self.assertRaisesRegex(RuntimeError, "warning_lines=1"):
                    ci.checked([sys.executable, "-c", f"print({message!r})"], os.environ.copy(), "annotation", dioxus_progress=True)
                self.assertEqual(output.getvalue(), message + "\n")
                result = json.loads((Path(directory) / "ci-results/annotation.json").read_text())
                self.assertEqual(result["warning_lines"], 1)
                self.assertEqual(result["external_metadata_progress_warnings"], [])

    def test_window_state_method_cfg_exactly_matches_desktop_debug_caller(self):
        source = (ROOT / "vendor-dioxus-desktop/src/app.rs").read_text()
        self.assertIn('''        #[cfg(debug_assertions)]
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        app.connect_preserve_window_state_handler();''', source)
        self.assertIn('''    #[cfg(all(
        debug_assertions,
        any(target_os = "windows", target_os = "linux", target_os = "macos")
    ))]
    fn connect_preserve_window_state_handler''', source)


class RuntimeResourcesAndPackaging(unittest.TestCase):
    @staticmethod
    def builder():
        spec = importlib.util.spec_from_file_location("ci_builder_under_test", ROOT / "build.py")
        builder = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = builder
        spec.loader.exec_module(builder)
        return builder

    @staticmethod
    def arch_fixture_archive(builder, payload, archive, *, corruption=None):
        import tarfile
        with tarfile.open(archive, "w:gz") as package:
            for original in sorted(payload.rglob("*")):
                name = original.relative_to(payload).as_posix()
                member = package.gettarinfo(str(original), arcname=name)
                member.uid = member.gid = 0
                if corruption == "owner":
                    member.uid = 1000
                if corruption == "mode":
                    member.mode ^= 0o001
                if member.isfile():
                    raw = original.read_bytes() + (b"changed" if corruption == "bytes" else b"")
                    member.size = len(raw)
                    package.addfile(member, io.BytesIO(raw))
                else:
                    package.addfile(member)
            info = ("pkgname = ubseds-groundstation\npkgver = 0.3.1-1\narch = x86_64\n"
                    "depend = gtk3\ndepend = webkit2gtk-4.1\n")
            if corruption == "metadata":
                info = info.replace("depend = gtk3", "depend = invented")
            for name, raw in [(".PKGINFO", info.encode()), (".MTREE", b"fixture"), (".BUILDINFO", b"fixture")]:
                member = tarfile.TarInfo(name)
                member.uid = member.gid = 0
                member.size = len(raw)
                package.addfile(member, io.BytesIO(raw))
            if corruption == "extra":
                member = tarfile.TarInfo("unexpected")
                member.size = 0
                package.addfile(member, io.BytesIO())

    def test_arch_archive_validation_checks_real_bytes_paths_modes_ownership_and_metadata(self):
        builder = self.builder()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = root / "payload"
            (payload / "usr/bin").mkdir(parents=True)
            (payload / "usr/bin/frontend").write_bytes(b"retained app payload")
            (payload / "usr/bin/frontend").chmod(0o755)
            for corruption in [None, "owner", "mode", "bytes", "metadata", "extra"]:
                with self.subTest(corruption=corruption):
                    archive = root / "fixture.pkg.tar.gz"
                    self.arch_fixture_archive(builder, payload, archive, corruption=corruption)
                    if corruption is None:
                        builder._validate_arch_payload_archive(archive, payload, "0.3.1", "x86_64")
                    else:
                        with self.assertRaises(RuntimeError):
                            builder._validate_arch_payload_archive(archive, payload, "0.3.1", "x86_64")

    def test_optional_arch_tools_missing_skips_without_staging_or_deleting_other_formats(self):
        builder = self.builder()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            packages = [root / "fresh.AppImage", root / "fresh.deb"]
            for package in packages:
                package.write_bytes(b"completed artifact")
            with patch.object(builder, "_which_in_path", return_value=None), patch.object(builder, "_stage_linux_app_payload") as stage:
                builder.build_manual_arch_package(root, None, False)
                stage.assert_not_called()
            for package in packages:
                self.assertEqual(package.read_bytes(), b"completed artifact")

    def test_debian_rpm_stage_preserves_fresh_appimage(self):
        builder = self.builder()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            appimage = root / "fresh.AppImage"
            appimage.write_bytes(b"completed AppImage")
            with patch.object(builder, "dist_dir", return_value=root), patch.object(builder, "_which_in_path", return_value="/tools/dpkg-deb"), patch.object(builder, "_stage_linux_app_payload", side_effect=RuntimeError("staging sentinel")):
                with self.assertRaisesRegex(RuntimeError, "staging sentinel"):
                    builder.build_manual_linux_packages(root, None, False)
            self.assertEqual(appimage.read_bytes(), b"completed AppImage")

    def test_arch_internal_phase_refuses_unqualified_makepkg_before_staging(self):
        builder = self.builder()
        with patch.object(builder, "_which_in_path", side_effect=lambda name, _path: Path("/tools") / name), \
             patch.object(builder, "run_capture", return_value="makepkg (pacman) 7.0.0\n"), \
             patch.object(builder, "_stage_linux_app_payload") as stage:
            with self.assertRaisesRegex(RuntimeError, "qualified makepkg 6.0.2"):
                builder.build_manual_arch_package(ROOT, None, False)
            stage.assert_not_called()

    def test_arch_recipe_contains_only_prebuilt_copy_stage_and_real_dependencies(self):
        import re
        builder = self.builder()
        recipe = builder._arch_payload_recipe("0.3.1", "quoted ' description", "x86_64", Path("/owned/payload"))
        self.assertEqual(re.findall(r"^(\w+)\(\) \{", recipe, re.MULTILINE), ["package"])
        self.assertNotRegex(recipe, r"(?m)^source(?:_[^=]+)?=")
        self.assertIn('depends=("gtk3" "webkit2gtk-4.1")', recipe)
        self.assertIn('cp -a /owned/payload/. "$pkgdir/"', recipe)

    def test_arch_generator_uses_owned_db_and_validates_before_dist_copy(self):
        builder = self.builder()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            calls = []
            def stage(*_args):
                lease = tempfile.TemporaryDirectory(dir=root)
                payload = Path(lease.name) / "payload"
                (payload / "usr/bin").mkdir(parents=True)
                (payload / "usr/bin/frontend").write_bytes(b"original payload")
                return lease, payload
            def tool(argv, cwd, env=None):
                calls.append((argv, cwd, env))
                if argv[0].endswith("fakeroot"):
                    self.assertEqual(argv[1:], ["--", "/tools/makepkg", "-F", "--force"])
                    self.assertEqual(list((cwd / "pkg").iterdir()), [])
                    self.assertEqual(list((cwd / "validation-db/local").iterdir()), [])
                    self.assertIn('--dbpath', (cwd / "owned-pacman").read_text())
                    self.assertEqual(env["PKGEXT"], ".pkg.tar.gz")
                    self.arch_fixture_archive(builder, cwd / "pkgsrc", cwd / "ubseds-groundstation-0.3.1-1-x86_64.pkg.tar.gz")
            with patch.object(builder, "_which_in_path", side_effect=lambda name, _path: Path("/tools") / name), \
                 patch.object(builder, "run_capture", return_value="makepkg (pacman) 6.0.2\n"), \
                 patch.object(builder, "_stage_linux_app_payload", side_effect=stage), \
                 patch.object(builder, "_read_frontend_version", return_value="0.3.1"), \
                 patch.object(builder, "_read_workspace_description", return_value="frontend"), \
                 patch.object(builder, "_linux_pkg_arch", return_value="x86_64"), \
                 patch.object(builder, "dist_dir", return_value=root / "dist"), \
                 patch.object(builder, "run", side_effect=tool):
                builder.build_manual_arch_package(ROOT, None, False)
            self.assertEqual([Path(c[0][0]).name for c in calls], ["fakeroot", "pacman"])
            self.assertIn("-Qip", calls[-1][0])
            self.assertEqual(len(list((root / "dist").glob("*.pkg.tar.gz"))), 1)

    def test_literal_resource_globs_are_removed_but_actual_embedded_paths_remain(self):
        import re
        import tomllib
        config = tomllib.loads((ROOT / "Dioxus.toml").read_text())
        self.assertEqual(config["bundle"]["resources"], [])
        consumers = {
            "src/app.rs": ["../static/activation_gesture_guard.js", "../static/vendor/maplibre-gl/maplibre-gl.js", "../static/vendor/maplibre-gl/maplibre-gl.css", "../static/ground_map.js", "../static/station_discovery.js"],
            "src/telemetry_dashboard/antenna_aiming.rs": ["../../assets/browser/antenna_sensor.js", "../../assets/browser/antenna_sensor_snapshot.js"],
            "src/telemetry_dashboard/station_positions_admin.rs": ["../../assets/browser/station_location.js"],
            "src/main.rs": ["../assets/icon.png"],
        }
        for source, required in consumers.items():
            text = (ROOT / source).read_text()
            includes = re.findall(r'include_(?:str|bytes)!\("([^\"]+)"\)', text)
            self.assertTrue(set(required).issubset(includes), source)
        native = (ROOT / "src/native_assets.rs").read_text()
        for model in ["vehicle.glb", "gse-site.glb"]:
            self.assertIn('include_bytes!("../station_ui/rendering/models/' + model + '")', native)
        for javascript in ["GLTFLoader.js", "three.module.js", "vehicle-renderer.js", "saved-replay-player.js"]:
            self.assertIn('include_bytes!("../station_ui/rendering/three/' + javascript + '")', native)
        build = (ROOT / "build.rs").read_text()
        self.assertIn('.join("presets.json")', build)
        self.assertIn('out_dir.join("theme_presets.json")', build)
        self.assertIn('"/theme_presets.json"', (ROOT / "src/telemetry_dashboard/mod.rs").read_text())
        ios = (ROOT / "scripts/patch_plist.sh").read_text()
        self.assertIn('cp -f "$ASSETS_CAR_SRC" "$APP_DIR/Assets.car"', ios)

    def test_common_icons_preserve_png_and_ico_and_existing_mac_icns_patch(self):
        import tomllib
        icons = tomllib.loads((ROOT / "Dioxus.toml").read_text())["bundle"]["icon"]
        self.assertIn("icons/icon.png", icons)
        self.assertIn("icons/icon.ico", icons)
        self.assertTrue(all(Path(icon).suffix in {".png", ".ico"} for icon in icons))
        builder = self.builder()
        with tempfile.TemporaryDirectory() as directory, patch.object(builder.platform, "system", return_value="Linux"):
            import shutil
            root = Path(directory)
            (root / "assets").mkdir()
            for name in ["icon.png", "icon.ico"]:
                shutil.copyfile(ROOT / "assets" / name, root / "assets" / name)
            try:
                from PIL import Image
            except ImportError:
                self.skipTest("Actual local icon-generation check requires Pillow, provisioned by CI bootstrap")
            with contextlib.redirect_stderr(io.StringIO()) as errors:
                builder._ensure_bundle_icon_compat(root)
            self.assertEqual(errors.getvalue(), "")
            for icon in icons:
                self.assertTrue((root / icon).is_file(), icon)
                with Image.open(root / icon) as image:
                    self.assertGreater(image.width, 0)
        # Mac generation/copy stays in its existing independently exercised path.
        source = (ROOT / "build.py").read_text()
        self.assertIn('icon_dst = resources_dir / "icon.icns"', source)
        self.assertIn('shutil.copy2(icon_src, icon_dst)', source)

    def test_prepared_linuxdeploy_wins_over_unverified_cache_and_fallback_is_native(self):
        builder = self.builder()
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            cache = home / ".cache/tauri"
            cache.mkdir(parents=True)
            for name in ["linuxdeploy-x86_64.AppImage", "linuxdeploy-aarch64.AppImage"]:
                (cache / name).write_bytes(b"cached tool")
            prepared = home / "prepared-native-linuxdeploy"
            with patch.object(Path, "home", return_value=home), patch.object(builder, "_which_in_path", return_value=str(prepared)):
                self.assertEqual(builder._resolve_linuxdeploy(), prepared)
            with patch.object(Path, "home", return_value=home), patch.object(builder, "_which_in_path", return_value=None), patch.object(builder.platform, "machine", return_value="arm64"):
                self.assertEqual(builder._resolve_linuxdeploy(), cache / "linuxdeploy-aarch64.AppImage")

    def test_actual_linux_payload_metadata_preserves_app_desktop_and_icon_contents(self):
        import tomllib
        import xml.etree.ElementTree as ET
        builder = self.builder()
        cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]
        component_id = tomllib.loads((ROOT / "Dioxus.toml").read_text())["bundle"]["identifier"].lower()
        desktop_bytes = []
        icons = []
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable = root / "input/groundstation_frontend"
            executable.parent.mkdir()
            executable.write_bytes(b"#!/bin/sh\nexit 0\n")
            executable.chmod(0o755)
            for appimage in [False, True]:
                with self.subTest(appimage=appimage), patch.object(builder, "_find_linux_app_binary", return_value=executable), patch.object(builder, "dist_dir", return_value=root / "empty-dist"), contextlib.redirect_stdout(io.StringIO()):
                    lease, package = builder._stage_linux_app_payload(ROOT, "x86_64-unknown-linux-gnu", False, appimage_mode=appimage)
                try:
                    desktop_name = component_id + ".desktop" if appimage else "ubseds-groundstation.desktop"
                    desktop = package / "usr/share/applications" / desktop_name
                    desktop_bytes.append(desktop.read_bytes())
                    binary = package / ("usr/bin/ubseds-groundstation" if appimage else "opt/ubseds-groundstation/ubseds-groundstation")
                    self.assertEqual(binary.read_bytes(), executable.read_bytes())
                    icon = package / "usr/share/icons/hicolor/256x256/apps/ubseds-groundstation.png"
                    icons.append(icon.read_bytes())
                    self.assertEqual((package / "usr/share/pixmaps/ubseds-groundstation.png").exists(), not appimage)
                    metadata = package / "usr/share/metainfo" / (component_id + ".appdata.xml")
                    tree = ET.fromstring(metadata.read_bytes())
                    self.assertEqual(tree.findtext("id"), component_id)
                    self.assertEqual(tree.findtext("launchable"), desktop_name)
                    self.assertEqual(tree.findtext("project_license"), "GPL-2.0-only")
                    self.assertEqual(tree.findtext("url"), cargo["repository"])
                    self.assertEqual(tree.findtext("description/p"), cargo["description"])
                    self.assertIsNone(tree.find("releases"))
                    self.assertIsNone(tree.find("screenshots"))
                finally:
                    lease.cleanup()
        self.assertEqual(desktop_bytes[0], desktop_bytes[1])
        self.assertEqual(icons[0], icons[1])

    def test_appstream_unknown_license_is_not_invented(self):
        builder = self.builder()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[package]\ndescription="Existing description"\n')
            (root / "LICENSE").write_text("Unrecognized license fixture")
            data = {"package": {"repository": "https://example.com/project", "license-file": "LICENSE"}}
            with patch.object(builder, "_workspace_root", return_value=root), patch.object(builder, "_load_toml_file", return_value=data):
                with self.assertRaisesRegex(ValueError, "explicit SPDX"):
                    builder._write_linux_appstream_metadata(ROOT, root / "output", "existing.desktop")
            self.assertFalse((root / "output").exists())

    def test_actual_appimage_command_uses_owned_hicolor_and_does_not_force_no_strip(self):
        builder = self.builder()
        for user_flag in [None, "1"]:
            with self.subTest(user_flag=user_flag), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                executable = root / "input/groundstation_frontend"
                executable.parent.mkdir()
                executable.write_bytes(b"#!/bin/sh\nexit 0\n")
                executable.chmod(0o755)
                captured = []
                def fake_tool(argv, **kwargs):
                    captured.append((argv, kwargs["env"]))
                    (Path(kwargs["cwd"]) / "fixture.AppImage").write_bytes(b"packaging fixture")
                environment = {} if user_flag is None else {"NO_STRIP": user_flag}
                with patch.dict(os.environ, environment, clear=True), patch.object(builder, "_resolve_linuxdeploy", return_value=Path("/prepared/linuxdeploy")), patch.object(builder, "_find_linux_app_binary", return_value=executable), patch.object(builder, "dist_dir", return_value=root / "dist"), patch.object(builder, "run", side_effect=fake_tool), contextlib.redirect_stdout(io.StringIO()):
                    builder.build_manual_appimage(ROOT, "x86_64-unknown-linux-gnu", False)
                self.assertEqual(len(captured), 1)
                argv, environment = captured[0]
                self.assertNotIn("--icon-file", argv)
                self.assertIn("--desktop-file", argv)
                self.assertEqual(environment.get("NO_STRIP"), user_flag)
                self.assertEqual(environment["APPIMAGE_EXTRACT_AND_RUN"], "1")
                self.assertTrue((root / "dist/UBSEDS GS_x86_64.AppImage").is_file())
        self.assertNotIn('env.setdefault("NO_STRIP", "1")', (ROOT / "build.py").read_text())

    def test_no_legacy_input_is_a_successful_noop_and_real_rename_failure_propagates(self):
        builder = self.builder()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            root.joinpath("UBSEDS GS").write_bytes(b"retained application")
            with patch.object(builder, "dist_dir", return_value=root), contextlib.redirect_stdout(io.StringIO()) as console, contextlib.redirect_stderr(io.StringIO()) as errors:
                builder.rename_windows_linux_artifacts(root, "linux")
            self.assertEqual(root.joinpath("UBSEDS GS").read_bytes(), b"retained application")
            self.assertIn("No legacy", console.getvalue())
            self.assertEqual(errors.getvalue(), "")
            root.joinpath("GroundstationFrontend").write_bytes(b"legacy application")
            with patch.object(builder, "dist_dir", return_value=root), patch.object(Path, "rename", side_effect=PermissionError("fixture denied")), contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(PermissionError):
                    builder.rename_windows_linux_artifacts(root, "linux")


if __name__ == "__main__":
    unittest.main()

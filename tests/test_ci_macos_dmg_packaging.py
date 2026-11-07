"""Disk-image packaging controls; fake native tools, no macOS qualification."""
import contextlib
import importlib.util
import io
from pathlib import Path
import os
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("dmg_builder", Path(__file__).resolve().parents[1] / "build.py")
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class MacosDmgPackaging(unittest.TestCase):
    def app(self, root):
        app = root / "dist" / build.APP_BUNDLE_NAME
        binary = app / "Contents/MacOS/groundstation_frontend"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"debug executable including original symbols")
        binary.chmod(0o755)
        return app, binary

    def test_sparse_logical_payload_and_external_symlink_are_budgeted_without_following(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            source.mkdir()
            with (source / "sparse-debug").open("wb") as f:
                f.truncate(16 << 20)
            external = root / "external"
            external.write_bytes(b"not payload")
            (source / "alias").symlink_to(external)
            allocation = build._macos_dmg_allocation(source)
            self.assertGreaterEqual(allocation["logical_bytes"], 16 << 20)
            self.assertGreater(allocation["image_mib"] * (1 << 20), 16 << 20)
            self.assertEqual(allocation["entries"], 3)
            self.assertLess(allocation["rounded_payload_bytes"], (16 << 20) + 8193)

    def test_real_staging_preserves_binary_symbols_permissions_and_symlink_then_verifies(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app, binary = self.app(root)
            calls, staged = [], []
            def tool(argv, **kwargs):
                calls.append(argv)
                if argv[:2] == ["hdiutil", "create"]:
                    src = Path(argv[argv.index("-srcfolder") + 1])
                    staged.append(src)
                    copied = src / build.APP_BUNDLE_NAME / binary.relative_to(app)
                    self.assertEqual(copied.read_bytes(), binary.read_bytes())
                    self.assertEqual(copied.stat().st_mode & 0o777, 0o755)
                    self.assertTrue((src / "Applications").is_symlink())
                    self.assertEqual(os.readlink(src / "Applications"), "/Applications")
                    self.assertEqual(argv[argv.index("-fs") + 1], "HFS+")
                    self.assertEqual(argv[argv.index("-format") + 1], "UDZO")
                    self.assertGreater(int(argv[argv.index("-size") + 1][:-1]), 64)
                    Path(argv[-1]).write_bytes(b"fixture compressed image")
            with patch.object(build, "run", side_effect=tool), contextlib.redirect_stdout(io.StringIO()):
                result = build.rebuild_macos_dmg(root)
            self.assertEqual(result.read_bytes(), b"fixture compressed image")
            self.assertEqual([c[:2] for c in calls], [["df", "-Pk"], ["du", "-sk"],
                             ["hdiutil", "create"], ["hdiutil", "imageinfo"], ["hdiutil", "verify"]])
            self.assertFalse(staged[0].exists())
            self.assertEqual(binary.read_bytes(), b"debug executable including original symbols")

    def test_failed_create_propagates_and_removes_only_owned_staging(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _, binary = self.app(root)
            staged = []
            def tool(argv, **kwargs):
                if argv[:2] == ["hdiutil", "create"]:
                    staged.append(Path(argv[argv.index("-srcfolder") + 1]))
                    raise subprocess.CalledProcessError(1, argv)
            with patch.object(build, "run", side_effect=tool), contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(subprocess.CalledProcessError):
                    build.rebuild_macos_dmg(root)
            self.assertFalse(staged[0].exists())
            self.assertTrue(binary.exists())

    def test_missing_image_and_failed_verification_are_fatal(self):
        for produced in [False, True]:
            with self.subTest(produced=produced), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.app(root)
                def tool(argv, **kwargs):
                    if produced and argv[:2] == ["hdiutil", "create"]:
                        Path(argv[-1]).write_bytes(b"fixture")
                    if argv[:2] == ["hdiutil", "verify"]:
                        raise subprocess.CalledProcessError(1, argv)
                with patch.object(build, "run", side_effect=tool), contextlib.redirect_stdout(io.StringIO()):
                    with self.assertRaises(subprocess.CalledProcessError if produced else FileNotFoundError):
                        build.rebuild_macos_dmg(root)

    def test_build_profiles_and_no_sign_still_produce_app_and_requested_dmg(self):
        for debug, include in [(True, True), (False, True), (True, False)]:
            with self.subTest(debug=debug, include=include), tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
                root = Path(directory)
                stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
                for name in ["_ensure_bundle_icon_compat", "_clear_dioxus_bundle_identity_cache", "_patch_macos_bundle_icon"]:
                    stack.enter_context(patch.object(build, name))
                for name in ["is_container", "in_docker_build"]:
                    stack.enter_context(patch.object(build, name, return_value=False))
                stack.enter_context(patch.object(build, "_ensure_wasm_bindgen_cli", return_value=None))
                stack.enter_context(patch.object(build, "_find_dx", return_value=Path("/fixture/dx")))
                stack.enter_context(patch.object(build, "_installed_dx_version", return_value=None))
                stack.enter_context(patch.object(build, "_workspace_dioxus_versions", return_value=[]))
                stack.enter_context(patch.object(build, "_dx_version_line_filter", return_value=None))
                commands = []
                def tool(argv, **kwargs):
                    commands.append(argv)
                    self.app(root)  # Actual produced bundle remains a debug payload.
                stack.enter_context(patch.object(build, "run", side_effect=tool))
                dmg = stack.enter_context(patch.object(build, "rebuild_macos_dmg"))
                sign = stack.enter_context(patch.object(build, "sign_macos_app_and_dmg"))
                build.build_frontend(root, "macos", rust_target="x86_64-apple-darwin", debug_mode=debug,
                                     sign_macos=False, macos_include_dmg=include)
                self.assertEqual(commands[0][commands[0].index("--package-types") + 1], "macos")
                self.assertEqual("--release" in commands[0], not debug)
                self.assertNotIn("strip", commands[0])
                self.assertEqual(dmg.call_count, int(include))
                sign.assert_not_called()

    def test_signing_order_stays_app_then_image_then_image_signature(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.app(root)
            image = root / "dist/image.dmg"
            image.write_bytes(b"fixture")
            events = []
            with patch.object(build.platform, "system", return_value="Darwin"), \
                 patch.object(build, "_pick_codesign_identity", return_value="fixture identity"), \
                 patch.object(build, "_macos_entitlements_path", return_value=None), \
                 patch.object(build, "run", side_effect=lambda *a, **k: events.append("app signed")), \
                 patch.object(build, "rebuild_macos_dmg", side_effect=lambda *a: events.append("image created") or image), \
                 patch.object(build, "sign_macos_dmg", side_effect=lambda *a: events.append("image signed")), \
                 contextlib.redirect_stdout(io.StringIO()):
                build.sign_macos_app_and_dmg(root)
            self.assertEqual(events, ["app signed", "image created", "image signed"])


if __name__ == "__main__":
    unittest.main()

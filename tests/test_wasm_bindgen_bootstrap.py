import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("frontend_build_bootstrap", Path(__file__).parents[1] / "build.py")
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class WasmBindgenBootstrapTests(unittest.TestCase):
    def check_install(self, existing):
        tool = Path("/tmp/test-tools/wasm-bindgen")
        versions = ["0.2.117", "0.2.118"] if existing else ["0.2.118"]
        with patch.object(build, "_required_wasm_bindgen_cli_version", return_value="0.2.118"), patch.object(build, "_find_wasm_bindgen", side_effect=[tool if existing else None, tool]), patch.object(build, "_wasm_bindgen_cli_version", side_effect=versions), patch.object(build, "run") as run:
            self.assertEqual(build._ensure_wasm_bindgen_cli(Path("/tmp/frontend"), {}), tool)
        command = run.call_args.args[0]
        self.assertNotIn("--locked", command)
        self.assertEqual(command[command.index("--version") + 1], "=0.2.118")
        self.assertEqual("--force" in command, existing)

    def test_new_install_resolves_non_yanked_dependencies_with_exact_tool_version(self):
        self.check_install(False)

    def test_replacement_install_uses_same_policy(self):
        self.check_install(True)

    def test_docker_tool_installers_do_not_force_yanked_published_lockfiles(self):
        docker=(Path(__file__).parents[1]/"Dockerfile.builder").read_text()
        installs=[line for line in docker.splitlines() if "cargo install" in line]
        self.assertEqual(len(installs),2)
        for line in installs:
            self.assertNotIn("--locked",line)
            self.assertIn("--version",line)

    def test_matching_tool_is_not_reinstalled(self):
        tool = Path("/tmp/test-tools/wasm-bindgen")
        with patch.object(build, "_required_wasm_bindgen_cli_version", return_value="0.2.118"), patch.object(build, "_find_wasm_bindgen", return_value=tool), patch.object(build, "_wasm_bindgen_cli_version", return_value="0.2.118"), patch.object(build, "run") as run:
            self.assertEqual(build._ensure_wasm_bindgen_cli(Path("/tmp/frontend"), {}), tool)
            run.assert_not_called()

    def test_wrong_installed_version_fails_instead_of_using_incompatible_abi(self):
        tool = Path("/tmp/test-tools/wasm-bindgen")
        with patch.object(build, "_required_wasm_bindgen_cli_version", return_value="0.2.118"), patch.object(build, "_find_wasm_bindgen", side_effect=[None, tool]), patch.object(build, "_wasm_bindgen_cli_version", return_value="0.2.119"), patch.object(build, "run"):
            with self.assertRaisesRegex(RuntimeError, "expected 0.2.118"):
                build._ensure_wasm_bindgen_cli(Path("/tmp/frontend"), {})

    def test_ios_bootstrap_and_retry_keep_host_environment(self):
        class EndOfBuild(Exception):
            pass

        tool = Path("/tmp/test-tools/wasm-bindgen")
        for target in ("aarch64-apple-ios", "aarch64-apple-ios-sim", "x86_64-apple-ios"):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                original_flags = "-C opt-level=2"
                with patch.dict(os.environ, {"RUSTFLAGS": original_flags}), \
                     patch.object(build, "is_container", return_value=False), \
                     patch.object(build, "in_docker_build", return_value=False), \
                     patch.object(build, "clear_ios_staging_bundle"), \
                     patch.object(build, "clear_ios_dx_bundle_cache"), \
                     patch.object(build, "_ensure_wasm_bindgen_cli", return_value=tool) as bootstrap, \
                     patch.object(build, "_find_dx", return_value=Path("/tmp/dx")), \
                     patch.object(build, "_installed_dx_version", return_value=None), \
                     patch.object(build, "_workspace_dioxus_versions", return_value=[]), \
                     patch.object(build, "_dx_version_line_filter", return_value=None), \
                     patch.object(build, "_tail_log_text", return_value="wasm-bindgen version mismatch"), \
                     patch.object(build, "_looks_like_wasm_bindgen_failure", return_value=True), \
                     patch.object(build, "_extract_wasm_bindgen_version_hint", return_value=None), \
                     patch.object(build, "run", side_effect=[subprocess.CalledProcessError(1, ["dx"]), EndOfBuild]) as run:
                    with self.assertRaises(EndOfBuild):
                        build.build_frontend(root, "ios", rust_target=target)
                    self.assertEqual(bootstrap.call_count, 2)
                    for call in bootstrap.call_args_list:
                        self.assertEqual(call.args[1]["RUSTFLAGS"], original_flags)
                    for call in run.call_args_list:
                        app_flags = call.kwargs["env"]["RUSTFLAGS"]
                        self.assertIn("link-arg=-target", app_flags)
                        self.assertIn(original_flags, app_flags)
                    self.assertEqual(os.environ["RUSTFLAGS"], original_flags)


if __name__ == "__main__":
    unittest.main()

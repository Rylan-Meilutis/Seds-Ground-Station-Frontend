import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import patch, Mock
spec = importlib.util.spec_from_file_location("frontend_targets", Path(__file__).parents[1] / "scripts/build_targets.py")
targets = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = targets
spec.loader.exec_module(targets)
class TargetTests(unittest.TestCase):
    def test_all_excludes_simulators_and_all_sims_includes_every_target(self):
        regular = targets.expand_targets("all")
        self.assertTrue(regular)
        self.assertTrue(all(not targets.TARGETS[n].simulator for n in regular))
        self.assertEqual(targets.expand_targets("all,sims"), list(targets.TARGETS))
        self.assertEqual(targets.expand_targets("web,web,linux"), ["web", "linux"])
    def test_unknown_target_is_not_silently_skipped(self):
        with self.assertRaises(ValueError): targets.expand_targets("typo")
    def test_docker_commands_keep_paths_and_arguments_separate(self):
        args = targets.parse_args(["--docker", "--target", "linux-arm64"])
        commands = targets.docker_commands(Path("/tmp/UI with spaces"), "linux-arm64", args)
        self.assertIn("linux/arm64", commands[0])
        self.assertIn("type=bind,source=/tmp/UI with spaces,target=/work", commands[1])
        self.assertNotIn("--docker", commands[1][commands[1].index("build.py")+1:])
        self.assertIn("CARGO_TARGET_DIR=/work/target/docker/linux-arm64", commands[1])
    def test_list_does_not_build(self):
        builder = Mock()
        self.assertTrue(targets.handle_target_args(["--target", "list"], Path("/tmp"), builder, Mock()))
        builder.assert_not_called()
    def test_dry_run_does_not_start_containers(self):
        with patch.object(targets.subprocess, "run") as run:
            targets.handle_target_args(["--target", "web,linux", "--docker", "--dry-run"], Path("/tmp"), Mock(), Mock())
            run.assert_not_called()
    def test_batch_continues_after_legacy_builder_exits(self):
        import tempfile
        with tempfile.TemporaryDirectory() as root, patch.object(targets.subprocess, "run"), patch.object(targets.platform, "system", return_value="Darwin"):
            builder = Mock(side_effect=[SystemExit(1), None])
            with self.assertRaises(SystemExit):
                targets.handle_target_args(["--target", "macos,macos-intel", "--no-sign"], Path(root), builder, Mock())
            self.assertEqual(builder.call_count, 2)

    def test_unavailable_native_targets_fail_instead_of_succeeding(self):
        with patch.object(targets.platform, "system", return_value="Linux"):
            with self.assertRaises(SystemExit):
                targets.handle_target_args(["--target", "ios", "--dry-run"], Path("/tmp"), Mock(), Mock())
if __name__ == "__main__": unittest.main()

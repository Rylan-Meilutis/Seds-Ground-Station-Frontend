"""Hosted failure regressions with fake tools only; no platform-build claim."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]

def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module

ci = load("platform_blocker_ci", "scripts/ci/frontend.py")
builder = load("platform_blocker_builder", "build.py")

class HostedPlatformBlockers(unittest.TestCase):
    def fixture_release(self, data=b"inert fixture JAR"):
        return {**ci.BUNDLETOOL, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}

    def install(self, root, release, *, reported="1.18.3", payload=b"inert fixture JAR"):
        calls = []
        def download(url, path):
            self.assertEqual(url, release["url"])
            path.write_bytes(payload)
        def checked(argv, env, label):
            calls.append(argv)
            self.assertEqual(label, "bundletool-version")
            self.assertEqual(argv[:2], ["java", "-jar"])
            self.assertEqual(argv[-1], "version")
            self.assertEqual(Path(argv[2]).read_bytes(), payload)
            logs = root / "ci-results"
            logs.mkdir(exist_ok=True)
            (logs / "bundletool-version.log").write_text(reported + "\n")
        with patch.object(ci, "ROOT", root), patch.object(ci, "BUNDLETOOL", release), \
             patch.object(ci, "download", side_effect=download), \
             patch.object(ci, "checked", side_effect=checked), \
             patch.object(ci.shutil, "which", return_value="/fixture/java"):
            env = ci.ci_env()
            ci.install_bundletool(env)
        return calls

    def test_verified_release_is_published_and_actual_builder_resolves_same_ci_jar(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            release = self.fixture_release()
            calls = self.install(root, release)
            self.assertEqual(len(calls), 1)
            with patch.object(ci, "ROOT", root), patch.object(ci, "BUNDLETOOL", release):
                env = ci.ci_env()
                ci.verify_bundletool(env)
                with patch.dict(os.environ, env), patch.object(builder.shutil, "which", return_value=None):
                    self.assertEqual(builder._resolve_bundletool_command(),
                        ["java", "-jar", str(root / ".ci-tools/bundletool/bundletool.jar")])
            receipt = json.loads((root / "ci-results/bundletool-release.json").read_text())
            self.assertEqual(receipt["sha256"], release["sha256"])
            self.assertEqual(receipt["reported"], release["version"])

    def test_corrupt_download_never_runs_java_or_publishes_tool(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            release = self.fixture_release()
            with patch.object(ci, "ROOT", root), patch.object(ci, "BUNDLETOOL", release), \
                 patch.object(ci, "download", side_effect=lambda _url, p: p.write_bytes(b"corrupt")), \
                 patch.object(ci, "checked") as checked, patch.object(ci.shutil, "which", return_value="java"):
                with self.assertRaisesRegex(RuntimeError, "size/checksum mismatch"):
                    ci.install_bundletool(ci.ci_env())
                checked.assert_not_called()
            self.assertFalse((root / ".ci-tools/bundletool").exists())

    def test_version_mismatch_never_publishes_tool(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(RuntimeError, "version mismatch"):
                self.install(root, self.fixture_release(), reported="0.0.0")
            self.assertFalse((root / ".ci-tools/bundletool").exists())

    def test_changed_tool_or_foreign_environment_is_refused_before_build(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            release = self.fixture_release()
            self.install(root, release)
            with patch.object(ci, "ROOT", root), patch.object(ci, "BUNDLETOOL", release):
                env = ci.ci_env()
                wrong = {**env, "BUNDLETOOL_JAR": "/foreign/tool.jar"}
                with self.assertRaisesRegex(RuntimeError, "changed prepared"):
                    ci.verify_bundletool(wrong)
                Path(env["BUNDLETOOL_JAR"]).write_bytes(b"changed")
                with self.assertRaisesRegex(RuntimeError, "changed prepared"):
                    ci.verify_bundletool(env)

    def test_rpm_manifest_covers_every_installed_fixture_file_including_metainfo(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            lease = tempfile.TemporaryDirectory(dir=root)
            staged = Path(lease.name) / "payload"
            installed = ["usr/bin/ubseds-groundstation", "opt/ubseds-groundstation/data.bin",
                "usr/share/applications/ubseds-groundstation.desktop",
                "usr/share/pixmaps/ubseds-groundstation.png",
                "usr/share/icons/hicolor/256x256/apps/ubseds-groundstation.png",
                "usr/share/metainfo/com.ubseds.gs26.appdata.xml"]
            for name in installed:
                p = staged / name
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_bytes(b"inert staged file")
            observed = []
            def run(argv, **_kwargs):
                if argv[0].endswith("rpmbuild"):
                    spec = Path(argv[2])
                    text = spec.read_text()
                    entries = [x for x in text.split("%files\n", 1)[1].splitlines() if x]
                    observed.extend(entries)
                    payload = spec.parents[2] / "rpm-root"
                    for p in payload.rglob("*"):
                        if p.is_file():
                            name = "/" + p.relative_to(payload).as_posix()
                            self.assertTrue(any(name == e or name.startswith(e + "/") for e in entries), name)
                    self.assertNotIn("unpackaged_files_terminate_build", text)
                    artifact = spec.parents[1] / "RPMS/x86_64/fixture.rpm"
                    artifact.parent.mkdir(parents=True)
                    artifact.write_bytes(b"fake rpmbuild output")
            with patch.object(builder, "_which_in_path", side_effect=lambda name, _path: "/tools/" + name), \
                 patch.object(builder, "_stage_linux_app_payload", return_value=(lease, staged)), \
                 patch.object(builder, "_read_frontend_version", return_value="0.3.1"), \
                 patch.object(builder, "_read_workspace_description", return_value="Ground station"), \
                 patch.object(builder, "_linux_appstream_id", return_value="com.ubseds.gs26"), \
                 patch.object(builder, "_linux_architecture", return_value=("amd64", "x86_64")), \
                 patch.object(builder, "dist_dir", return_value=root / "dist"), \
                 patch.object(builder, "run", side_effect=run):
                builder.build_manual_linux_packages(root, None, False)
            self.assertIn("/usr/share/metainfo/com.ubseds.gs26.appdata.xml", observed)
            self.assertTrue((root / "dist" / f"{builder.APP_NAME}_x86_64.rpm").is_file())

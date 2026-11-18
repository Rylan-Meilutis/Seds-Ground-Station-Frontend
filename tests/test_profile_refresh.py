"""Installed development profiles: both Xcode locations, newest wins over a stale local copy."""

import datetime as dt
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
spec = importlib.util.spec_from_file_location("profile_refresh_build", ROOT / "build.py")
build = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = build
spec.loader.exec_module(build)

WATCH_ID = "com.UBSEDS.GS26.watchkitapp"


def info(created_day, bundle=WATCH_ID, devices=("WATCHUDID",), dev=True):
    return {
        "CreationDate": dt.datetime(2026, 11, created_day),
        "ExpirationDate": dt.datetime(2027, 11, created_day),
        "ProvisionedDevices": list(devices),
        "Entitlements": {"get-task-allow": dev, "application-identifier": f"S94GMZ877L.{bundle}"},
    }


class ProfileRefreshTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.home = Path(self.tmp.name)
        self.new_dir = self.home / "Library/Developer/Xcode/UserData/Provisioning Profiles"
        self.old_dir = self.home / "Library/MobileDevice/Provisioning Profiles"
        self.new_dir.mkdir(parents=True)
        self.old_dir.mkdir(parents=True)
        self.frontend = self.home / "frontend"
        self.frontend.mkdir()
        self.infos = {}

    def tearDown(self):
        self.tmp.cleanup()

    def add(self, path, profile_info):
        path.write_bytes(b"x")
        self.infos[str(path)] = profile_info
        return path

    def patched(self):
        return (
            mock.patch.object(build.Path, "home", return_value=self.home),
            mock.patch.object(build, "_decode_mobileprovision", side_effect=lambda p, _f: self.infos[str(p)]),
            mock.patch.object(build, "_profile_matches_bundle",
                              side_effect=lambda i, b: i["Entitlements"]["application-identifier"].endswith("." + b)),
        )

    def test_finds_profiles_in_the_xcode16_directory(self):
        found = self.add(self.new_dir / "a.mobileprovision", info(10))
        with self.patched()[0], self.patched()[1], self.patched()[2]:
            self.assertEqual(build._find_installed_ios_development_profile(self.frontend, WATCH_ID), found)

    def test_newest_creation_date_wins_across_both_directories(self):
        self.add(self.old_dir / "old.mobileprovision", info(5))
        newest = self.add(self.new_dir / "new.mobileprovision", info(17))
        with self.patched()[0], self.patched()[1], self.patched()[2]:
            self.assertEqual(build._find_installed_ios_development_profile(self.frontend, WATCH_ID), newest)

    def test_distribution_and_other_bundles_are_ignored(self):
        self.add(self.new_dir / "dist.mobileprovision", info(18, dev=False))
        self.add(self.new_dir / "phone.mobileprovision", info(18, bundle="com.UBSEDS.GS26"))
        with self.patched()[0], self.patched()[1], self.patched()[2]:
            self.assertIsNone(build._find_installed_ios_development_profile(self.frontend, WATCH_ID))

    def test_newer_installed_profile_replaces_stale_local_copy(self):
        local = self.add(self.frontend / "Groundstation_watch_dev.mobileprovision", info(3))
        newer = self.add(self.new_dir / "regenerated.mobileprovision", info(17))
        with self.patched()[0], self.patched()[1], self.patched()[2]:
            self.assertEqual(build._prefer_newer_installed_profile(self.frontend, local, WATCH_ID, "watch development"), newer)

    def test_local_copy_kept_when_it_is_newest(self):
        local = self.add(self.frontend / "Groundstation_watch_dev.mobileprovision", info(17))
        self.add(self.new_dir / "older.mobileprovision", info(3))
        with self.patched()[0], self.patched()[1], self.patched()[2]:
            self.assertEqual(build._prefer_newer_installed_profile(self.frontend, local, WATCH_ID, "watch development"), local)


if __name__ == "__main__":
    unittest.main()

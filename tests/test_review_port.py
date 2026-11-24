"""The offline review's copies of GroundStation26 code match a checkout
(scripts/check_review_port.py); skipped without one."""
import importlib.util
import os
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("check_review_port", ROOT / "scripts" / "check_review_port.py")
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)


class ReviewPortTest(unittest.TestCase):
    def test_every_copy_exists(self):
        for name in check.COPIES:
            self.assertTrue((check.REVIEW / name).is_file(), name)

    def test_copies_match_the_station(self):
        gs = Path(os.environ.get("GROUNDSTATION26", check.DEFAULT_GS))
        if not (gs / "backend" / "src" / "flight_review.rs").is_file():
            self.skipTest(f"no GroundStation26 checkout at {gs}")
        self.assertEqual(check.differences(gs), [])


if __name__ == "__main__":
    unittest.main()

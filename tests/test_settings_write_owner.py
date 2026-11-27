"""A station write needs an owner that may write.

`put_captured`, `patch_captured`, `delete_captured` and `post_captured` check
`ExportOwner::check_ordinary`, which an observer owner
(`capture_observer_selected`) never passes: the proximity lock and recovery
search settings were refused with "Original ordinary policy owner is missing"
for every administrator. Writes take their owner from
`station_settings_request::use_factory()` (or `capture_ordinary_selected`).
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WRITE = re.compile(r"\b(?:put|patch|delete|post)_captured\b(?!_independent)")
OBSERVER = re.compile(r"\bcapture_observer_selected\b")


def observer_writes(text):
    """(line, write line) where an observer owner is followed by a write that
    requires an ordinary owner within the next few lines of the same block."""
    lines = text.splitlines()
    found = []
    for index, line in enumerate(lines):
        if not OBSERVER.search(line):
            continue
        for later in lines[index + 1:index + 4]:
            if WRITE.search(later):
                found.append((index + 1, later.strip()))
                break
    return found


class SettingsWriteOwner(unittest.TestCase):
    def test_no_write_uses_an_observer_owner(self):
        problems = []
        for path in sorted((ROOT / "src").rglob("*.rs")):
            if path.name == "station_settings_request.rs" or path.name.endswith("_tests.rs"):
                continue
            for line, write in observer_writes(path.read_text(encoding="utf-8")):
                problems.append(f"{path.relative_to(ROOT)}:{line}: {write}")
        self.assertEqual(problems, [])

    def test_the_check_sees_the_old_proximity_save(self):
        old = """let owner = super::recording_download::ExportOwner::capture_observer_selected(|| true);
busy.set(true);
spawn(async move {
    let reply = super::station_settings_request::put_captured::<_, Status>(SETTINGS_PATH, &body, owner).await;"""
        self.assertEqual(len(observer_writes(old)), 1)
        independent = """let owner = ExportOwner::capture_observer_selected(|| true);
station_settings_request::post_captured_independent(&path, body, owner)"""
        self.assertEqual(observer_writes(independent), [])

    def test_both_views_write_through_the_settings_factory(self):
        for name in ["proximity_lock_view.rs", "recovery_search_view.rs"]:
            text = (ROOT / "src/telemetry_dashboard" / name).read_text(encoding="utf-8")
            self.assertIn("station_settings_request::use_factory()", text, name)
            self.assertNotIn("capture_observer_selected", text, name)


if __name__ == "__main__":
    unittest.main()

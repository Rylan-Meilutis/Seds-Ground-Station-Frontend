#!/usr/bin/env python3
"""Check the offline review's copies of GroundStation26 code.

`src/telemetry_dashboard/local_station/exports/review/` holds exact copies of
some GroundStation26 backend files (the rest is ported; the golden tests
compare every answer with the station's). This compares the copies with a
GroundStation26 checkout; exit status 1 means a copy differs.

    python3 scripts/check_review_port.py            # compare
    python3 scripts/check_review_port.py --update   # copy the station's files in

The checkout is --gs, else $GROUNDSTATION26, else ../GroundStation26 next to
this checkout.
"""
import argparse
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REVIEW = ROOT / "src" / "telemetry_dashboard" / "local_station" / "exports" / "review"
DEFAULT_GS = ROOT.parent / "GroundStation26"

# Copy here -> (file in backend/src, the marker line it stops before, or None for the whole file).
COPIES = {
    "zip.rs": ("flight_review/zip.rs", None),
    "plot.rs": ("flight_review/plot.rs", None),
    "thrust.rs": ("burn_report/thrust.rs", None),
    "merge.rs": ("burn_report/merge.rs", None),
    # The model files are the bundle's (models/hybrid_motor/), not the build's.
    "hybrid.rs": ("apogee_planner/hybrid.rs", "// ------------------------------------------------------------------ committed model files"),
}


def expected(gs: Path, source: str, marker: str | None) -> str:
    text = (gs / "backend" / "src" / source).read_text(encoding="utf-8")
    if marker is None:
        return text
    lines = text.splitlines(keepends=True)
    for i, line in enumerate(lines):
        if line.rstrip("\n") == marker:
            return "".join(lines[:i])
    raise SystemExit(f"{source}: marker line not found: {marker}")


def differences(gs: Path) -> list[str]:
    problems = []
    for name, (source, marker) in COPIES.items():
        ours = REVIEW / name
        if not ours.is_file() or ours.read_text(encoding="utf-8") != expected(gs, source, marker):
            problems.append(f"{name} differs from GroundStation26 backend/src/{source}")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--gs", type=Path, help="GroundStation26 checkout")
    parser.add_argument("--update", action="store_true", help="replace the copies with the station's files")
    args = parser.parse_args()
    gs = args.gs or Path(os.environ.get("GROUNDSTATION26", DEFAULT_GS))
    if not (gs / "backend" / "src" / "flight_review.rs").is_file():
        print(f"No GroundStation26 checkout at {gs}", file=sys.stderr)
        return 2
    if args.update:
        for name, (source, marker) in COPIES.items():
            (REVIEW / name).write_text(expected(gs, source, marker), encoding="utf-8")
        print(f"Copied {len(COPIES)} file(s) from {gs}; run `cargo test offline_review`.")
        return 0
    problems = differences(gs)
    if problems:
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        print("Run with --update, then `cargo test offline_review`.", file=sys.stderr)
        return 1
    print(f"The review's copies match {gs}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

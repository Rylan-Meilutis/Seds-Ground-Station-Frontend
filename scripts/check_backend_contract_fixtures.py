#!/usr/bin/env python3
"""Compare the UI's backend contract snapshot with GroundStation26's fixtures.

The backend writes real responses to backend/tests/contract-fixtures; this
repository keeps a copy in tests/fixtures/backend-contract that the Rust
contract tests decode. Exit status 1 means the copy differs from the backend.

    python3 scripts/check_backend_contract_fixtures.py            # compare
    python3 scripts/check_backend_contract_fixtures.py --update   # refresh the copy

The backend directory is --backend, else $GS_BACKEND_CONTRACT_FIXTURES, else
../GroundStation26/backend/tests/contract-fixtures next to this checkout.
"""
import argparse
import filecmp
import os
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SNAPSHOT = ROOT / "tests" / "fixtures" / "backend-contract"
DEFAULT_BACKEND = ROOT.parent / "GroundStation26" / "backend" / "tests" / "contract-fixtures"


def json_files(directory: Path) -> set[str]:
    return {p.relative_to(directory).as_posix() for p in directory.rglob("*.json")}


def differences(backend: Path, snapshot: Path) -> list[str]:
    ours, theirs = json_files(snapshot), json_files(backend)
    problems = [f"missing from the snapshot: {name}" for name in sorted(theirs - ours)]
    problems += [f"no longer produced by the backend: {name}" for name in sorted(ours - theirs)]
    problems += [
        f"differs: {name}"
        for name in sorted(ours & theirs)
        if not filecmp.cmp(backend / name, snapshot / name, shallow=False)
    ]
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--backend", type=Path, help="GroundStation26 backend/tests/contract-fixtures directory")
    parser.add_argument("--update", action="store_true", help="replace the snapshot with the backend's fixtures")
    args = parser.parse_args()
    backend = args.backend or Path(os.environ.get("GS_BACKEND_CONTRACT_FIXTURES", DEFAULT_BACKEND))
    if not backend.is_dir() or not json_files(backend):
        print(f"No backend contract fixtures in {backend}", file=sys.stderr)
        return 2
    problems = differences(backend, SNAPSHOT) if SNAPSHOT.is_dir() else ["snapshot directory missing"]
    if args.update:
        if problems:
            shutil.rmtree(SNAPSHOT, ignore_errors=True)
            shutil.copytree(backend, SNAPSHOT, ignore=shutil.ignore_patterns("*.md"))
            print(f"Snapshot refreshed from {backend} ({len(problems)} change(s)).")
        else:
            print("Snapshot already matches the backend.")
        return 0
    if problems:
        print(f"Backend contract snapshot differs from {backend}:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        print("Run with --update, then `cargo test backend_contract -- --test-threads=1`.", file=sys.stderr)
        return 1
    print(f"Snapshot matches {backend}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

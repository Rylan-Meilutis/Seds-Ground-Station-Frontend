"""Every station route the UI requests is documented in docs/backend-contract.md.

Scans the production Rust sources (test modules and *_tests.rs files are
skipped) for string literals that name a station route (`/api/...`,
`/flightstate`, `/tiles/...`, `/ws`), normalises them (host, `{base}` and
query removed; any `{...}` placeholder is one wildcard segment) and checks
each matches a backticked path in the contract document. A new route without
a documented row fails here; add it to the right table in section 5 and to
the route index.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
DOC = ROOT / "docs" / "backend-contract.md"
ROUTE = re.compile(r"(/(?:api/|flightstate\b|tiles/|ws\b)[^\s\"?#]*)")
LITERAL = re.compile(r'"((?:[^"\\]|\\.)*)"')

# Literals that look like routes but are not requests the UI makes.
NOT_ROUTES = {
    "/api/auth",  # native_assets.rs: a path the asset handler must refuse
}


def strip_test_modules(text):
    """Remove `#[cfg(test)]` items (brace matched), keeping production code."""
    out, i = [], 0
    while True:
        at = text.find("#[cfg(test)]", i)
        if at < 0:
            out.append(text[i:])
            return "".join(out)
        out.append(text[i:at])
        brace = text.find("{", at)
        semi = text.find(";", at)
        if brace < 0 or (0 <= semi < brace):
            i = semi + 1 if semi >= 0 else len(text)
            continue
        depth, j = 0, brace
        while j < len(text):
            if text[j] == "{":
                depth += 1
            elif text[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        i = j + 1


def segments(path):
    path = path.split("?")[0].rstrip("/")
    return ["*" if "{" in part or "<" in part else part for part in path.split("/")[1:]]


def source_routes():
    routes = {}
    for file in sorted((ROOT / "src").rglob("*.rs")):
        if file.name.endswith("_tests.rs") or file.name == "tests.rs":
            continue
        text = strip_test_modules(file.read_text(encoding="utf-8"))
        for literal in LITERAL.findall(text):
            for match in ROUTE.findall(literal):
                route = match.rstrip(".,;:)")
                # Encoded or traversal inputs are security checks, not routes.
                if "%" in route or ".." in route or "\\" in route or route in NOT_ROUTES:
                    continue
                routes.setdefault(route, file.relative_to(ROOT))
    return routes


def documented():
    text = DOC.read_text(encoding="utf-8")
    paths = set()
    for code in re.findall(r"`([^`]+)`", text):
        for match in ROUTE.findall(code):
            paths.add(match.rstrip(".,;:)"))
    return [segments(p) for p in paths]


def matches(route, template):
    if len(route) != len(template):
        return False
    return all(a == b or a == "*" or b == "*" for a, b in zip(route, template))


class BackendContractDocs(unittest.TestCase):
    def test_every_requested_route_is_documented(self):
        templates = documented()
        self.assertGreater(len(templates), 150, "the contract document lists the routes")
        missing = []
        for route, file in source_routes().items():
            parts = segments(route)
            prefix = route.endswith("/")
            ok = any(matches(parts, t) or (prefix and t[: len(parts)] == parts) for t in templates)
            if not ok:
                missing.append(f"{route}  ({file})")
        self.assertEqual(missing, [], "routes requested by the UI but not in docs/backend-contract.md:\n  " + "\n  ".join(missing))

    def test_the_scan_finds_the_core_routes(self):
        found = {tuple(segments(r)) for r in source_routes()}
        for core in ["/api/layout", "/api/boards", "/flightstate", "/api/recent", "/ws"]:
            self.assertIn(tuple(segments(core)), found)

    def test_normalisation(self):
        self.assertEqual(segments("/api/recordings/{id}/csv"), ["api", "recordings", "*", "csv"])
        self.assertTrue(matches(segments("/api/recordings/{}/replay/window?upper_id={}"), segments("/api/recordings/{id}/replay/window")))
        self.assertFalse(matches(segments("/api/recordings/x/csv"), segments("/api/recordings/{id}/file")))


if __name__ == "__main__":
    unittest.main()

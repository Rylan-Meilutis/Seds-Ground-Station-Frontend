//! Backend contract fixtures: real GroundStation26 responses and websocket
//! frames, decoded here with the UI's own types.
//!
//! The backend writes them to `GroundStation26/backend/tests/contract-fixtures`
//! (its `contract_fixtures` test fails when they are stale). This repository
//! keeps a copy in `tests/fixtures/backend-contract`; set
//! `GS_CONTRACT_FIXTURES_DIR` to decode a backend checkout's files directly.
//! `scripts/check_backend_contract_fixtures.py` compares or refreshes the copy.
//! See `docs/backend-contract-tests.md`.
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) const DIR_ENV: &str = "GS_CONTRACT_FIXTURES_DIR";

pub(crate) fn dir() -> PathBuf {
    std::env::var_os(DIR_ENV).map(PathBuf::from).unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("backend-contract")
    })
}

/// The fixture's JSON, e.g. `fixture("station-modes/snapshot")`.
pub(crate) fn fixture(name: &str) -> Value {
    let path = dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("backend contract fixture {} unreadable: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("backend contract fixture {} is not JSON: {e}", path.display()))
}

/// Decodes a fixture with a UI type, naming the fixture on failure.
pub(crate) fn decode<T: serde::de::DeserializeOwned>(name: &str) -> T {
    serde_json::from_value(fixture(name)).unwrap_or_else(|e| {
        panic!("the UI cannot decode backend contract fixture {name}: {e}")
    })
}

/// Every fixture some UI test decodes. A fixture the backend adds must be
/// covered here too, so a new contract cannot go untested.
pub(crate) const COVERED: &[&str] = &[
    "action-policy/get",
    "alerts/ack",
    "alerts/history-clear",
    "alerts/list",
    "auth/challenge",
    "auth/session-anonymous",
    "auth/session-operator",
    "board-recovery/status-operator",
    "board-recovery/status-viewer",
    "network-diagnostics/radio-tx-budget",
    "radio/modem",
    "rrls/status",
    "rrls/status-relaying",
    "station-mode/status",
    "station-modes/confirm-accepted",
    "station-modes/prepare-in-process",
    "station-modes/prepare-restart",
    "station-modes/snapshot",
    "station-modes/snapshot-after-switch",
    "websocket/action-policy",
    "websocket/alert-ack-state",
    "websocket/station-mode-changed",
];

fn json_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            json_files(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            let name = path.strip_prefix(root).unwrap().with_extension("");
            out.push(name.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn every_backend_contract_fixture_is_decoded_by_a_ui_test() {
    let root = dir();
    let mut found = Vec::new();
    json_files(&root, &root, &mut found);
    found.sort();
    let mut covered = COVERED.to_vec();
    covered.sort();
    assert_eq!(
        found,
        covered,
        "fixtures in {} and the UI's covered list differ; add a decoding test for each new fixture",
        root.display()
    );
}

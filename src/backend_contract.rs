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
    "board-abort/status-hitl",
    "board-recovery/status-operator",
    "board-recovery/status-viewer",
    "fc-estimator/get",
    "flight-state/get-fill",
    // Flight plans and loss-of-signal prediction: flight_plans.rs and
    // los_prediction.rs `decodes_the_backend_fixtures`.
    "flight-plans/bundle-manifest",
    "flight-plans/list",
    "flight-plans/plan",
    "flight-plans/run-forbidden",
    "los-prediction/idle",
    "los-prediction/simulate",
    // map_3d.rs: the live track, the recorded trajectory and terrain, with phases.
    "flight-phases/live",
    "network-diagnostics/radio-tx-budget",
    "radio/modem",
    // Opened with no station in offline_preview/tests.rs and local_station/tests.rs.
    "replay-export/flight",
    // With replay-bundle/flight.gsreplay and flight.csv: opened with no station
    // and its CSV export compared with the station's (local_station/tests.rs).
    // Recovery search (backend_contract_tests::recovery_search_*).
    "recovery-search/cleared",
    "recovery-search/opt-in",
    "recovery-search/refused-clear-operator",
    "recovery-search/refused-not-opted-in",
    "recovery-search/refused-start-viewer",
    "recovery-search/started",
    "recovery-search/status",
    "recovery-search/stopped",
    "recovery-search/track-accepted",
    "recovery-search/track-duplicate",
    "replay-bundle/flight.manifest",
    "replay-trajectory/flight",
    // The station's exports of the bundled recording and of a report-cases
    // recording, answered offline and compared (local_station/tests.rs).
    "replay-bundle/exports/cases-calibration-history",
    "replay-bundle/exports/cases-report-preview",
    "replay-bundle/exports/cases-report-preview-window",
    "replay-bundle/exports/flight-report-preview",
    "replay-bundle/exports/flight-report-preview-window",
    "replay-bundle/exports/requests",
    // An SD-resolved recording's bundle (its manifest is the one inside
    // sd.gsreplay) and the station's previews of it (local_station/tests.rs).
    "replay-bundle/sd.manifest",
    "replay-bundle/exports/sd-report-preview",
    "replay-bundle/exports/sd-report-preview-window",
    // Custom-window reviews and review exports of three bundles, answered
    // offline and compared (local_station/tests.rs).
    "replay-bundle/review/fire-burn-summary",
    "replay-bundle/review/flight-launch-summary",
    "replay-bundle/review/requests",
    "requests/gse-self-test-confirmation",
    "rrls/status",
    "rrls/status-relaying",
    "station-mode/status",
    "station-modes/confirm-accepted",
    "station-modes/prepare-in-process",
    "station-modes/prepare-restart",
    "station-modes/snapshot",
    "station-modes/snapshot-after-switch",
    "terrain/heightmap",
    "terrain/heightmap-unavailable",
    "terrain/status-empty",
    "vehicle/geometry",
    "websocket/action-policy",
    "websocket/alert-ack-state",
    "websocket/fc-estimator",
    "websocket/flight-state-in-flight",
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

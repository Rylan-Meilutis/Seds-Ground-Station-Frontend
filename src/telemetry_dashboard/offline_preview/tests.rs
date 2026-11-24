//! The offline entry runs the live dashboard against a local station, and
//! leaves nothing selected or alive when it unmounts.
use super::*;

#[test]
fn bundles_and_replay_files_open_through_the_same_entry() {
    let bundle = include_bytes!("../../../tests/fixtures/backend-contract/replay-bundle/flight.gsreplay");
    let session = open_bytes("flight.gsreplay", bundle).expect("bundle opens");
    assert!(!session.is_demo());
    let text = include_str!("../../../tests/fixtures/backend-contract/replay-export/flight.json");
    let session = open_bytes("flight.seds-replay.json", text.as_bytes()).expect("replay file opens");
    assert!(!session.samples.is_empty());
    assert!(open_bytes("x.json", b"\xff\xfe").is_err());
}

#[test]
fn clock_text_counts_down_to_the_demo_ignition() {
    let station = LocalStation::new(demo::session(0), 0);
    assert_eq!(station.tclock_text(100_000), "T- 00:10.00", "before the countdown: the countdown length, as the header shows idle");
    assert_eq!(station.tclock_text(130_000), "T- 00:10.00");
    assert_eq!(station.tclock_text(135_250), "T- 00:04.75");
    assert_eq!(station.tclock_text(141_500), "T+ 00:01.50");
    assert_eq!(position_text(141_500, 0), "02:21.5");
}

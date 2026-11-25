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

#[test]
fn a_handed_over_file_opens_as_a_replay_without_the_demo() {
    let bundle = include_bytes!("../../../tests/fixtures/backend-contract/replay-bundle/flight.gsreplay");
    let before = local_station::live_sessions();
    let session = starting_session(None, None, Some(Ok(("flight.gsreplay".into(), bundle.to_vec())))).expect("bundle opens");
    assert!(!session.is_demo());
    assert_eq!(banner_title(&session), "Replay: flight.gsreplay");
    assert_eq!(local_station::live_sessions(), before + 1, "only the replay was built, no demo beside it");
    drop(session);

    // A file that does not open is reported; nothing (no demo) opens instead.
    let error = starting_session(None, None, Some(Ok(("broken.gsreplay".into(), b"\xff\xfe".to_vec())))).err().expect("refused");
    assert!(error.starts_with("Could not open broken.gsreplay"), "{error}");
    let error = starting_session(None, None, Some(Err("big.gsreplay is larger than 192 MiB".into()))).err().expect("rejected");
    assert!(error.contains("192 MiB"), "{error}");
    assert_eq!(local_station::live_sessions(), before);

    // With nothing handed over the route is demo mode (Explore offline).
    let demo = starting_session(None, None, None).expect("demo");
    assert!(demo.is_demo());
    assert_eq!(banner_title(&demo), "Demo mode — no station connected");
}

#[test]
fn the_connection_page_hand_off_reaches_the_replay_view_once() {
    // The connection page offers a picked file exactly as another app does;
    // the replay view takes it on entry, and only once.
    let _turn = crate::opened_file::TEST_TURN.lock().unwrap_or_else(|p| p.into_inner());
    let text = include_str!("../../../tests/fixtures/backend-contract/replay-export/flight.json");
    crate::opened_file::offer_from("flight.seds-replay.json".into(), text.as_bytes().to_vec(), None);
    let (opened, path) = take_opened().expect("waiting");
    assert_eq!(path, None);
    let session = starting_session(None, None, Some(opened)).expect("replay file opens");
    assert_eq!(banner_title(&session), "Replay: flight.seds-replay.json");
    assert!(take_opened().is_none(), "taken once");
}

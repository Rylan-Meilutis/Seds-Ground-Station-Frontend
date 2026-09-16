use super::*;
use std::rc::Rc;

fn populated_calibration() -> CalibrationFile {
    let mut cfg = CalibrationFile::default();
    let channel = cfg.channels.entry("LOADCELL".into()).or_default();
    channel.zero_raw = Some(1.0);
    channel.points = vec![
        CalibrationPoint { expected: 10.0, raw: 11.0 },
        CalibrationPoint { expected: 5.0, raw: 6.0 },
    ];
    channel.linear = ChannelLinear { m: Some(1.0), b: Some(-1.0) };
    cfg
}

#[test]
fn direction_clicks_preserve_saved_and_unsaved_calibration() {
    let mut dom = VirtualDom::new(|| rsx! { div {} });
    dom.rebuild_in_place();
    dom.in_scope(ScopeId::ROOT, || {
        for is_dirty in [false, true] {
            let original = populated_calibration();
            let cfg = Signal::new(original.clone());
            let dirty = Signal::new(is_dirty);
            let direction = Signal::new(CaptureDirection::Increasing);
            let saved_document = serde_json::to_string(&original).unwrap();
            for next in [CaptureDirection::Decreasing, CaptureDirection::Increasing, CaptureDirection::Decreasing] {
                let event = Event::new(Rc::new(()), true);
                choose_capture_direction(event.clone(), direction, next);
                assert_eq!(*direction.read(), next);
                assert_eq!(*cfg.read(), original);
                assert_eq!(*dirty.read(), is_dirty);
                assert_eq!(serde_json::to_string(&*cfg.read()).unwrap(), saved_document);
                assert!(!event.default_action_enabled(), "direction cannot submit a form");
                assert!(!event.propagates(), "direction cannot trigger parent actions");
            }
        }
    });
}

#[test]
fn descending_final_zero_keeps_all_weighted_measurements() {
    let mut cfg = populated_calibration();
    // Only the explicitly confirmed first capture starts a replacement.
    apply_sequence_capture(&mut cfg, "LOADCELL", CaptureMode::SequencePoint, 20.0, 22.0, true).unwrap();
    apply_sequence_capture(&mut cfg, "LOADCELL", CaptureMode::SequencePoint, 10.0, 12.0, false).unwrap();
    let points = cfg.channels["LOADCELL"].points.clone();
    apply_sequence_capture(&mut cfg, "LOADCELL", CaptureMode::SequenceZero, 0.0, 2.0, false).unwrap();
    assert_eq!(cfg.channels["LOADCELL"].points, points);
    assert_eq!(cfg.channels["LOADCELL"].zero_raw, Some(2.0));
    local_refit_channel(&mut cfg, "LOADCELL", "linear").unwrap();
    assert!((eval_fit_key(&cfg, "LOADCELL", 12.0).unwrap() - 10.0).abs() < 0.001);
}

#[test]
fn final_zero_is_not_a_tare_that_shifts_previous_measurements() {
    let mut cfg = populated_calibration();
    let points = cfg.channels["LOADCELL"].points.clone();
    apply_sequence_capture(&mut cfg, "LOADCELL", CaptureMode::SequenceZero, 0.0, 2.0, false).unwrap();
    assert_eq!(cfg.channels["LOADCELL"].points, points);
    assert_eq!(cfg.channels["LOADCELL"].zero_raw, Some(2.0));
}

#[test]
fn invalid_first_capture_never_erases_existing_calibration() {
    for (weight, raw) in [(0.0, 1.0), (f32::NAN, 1.0), (10.0, f32::NAN)] {
        let mut cfg = populated_calibration();
        let original = cfg.clone();
        assert!(apply_sequence_capture(&mut cfg, "LOADCELL", CaptureMode::SequencePoint, weight, raw, true).is_err());
        assert_eq!(cfg, original);
    }
}

#[test]
fn delayed_empty_save_response_cannot_erase_new_points() {
    let submitted = CalibrationFile::default();
    let mut current = populated_calibration();
    let original = current.clone();
    assert!(!accept_calibration_save(&mut current, &submitted, submitted.clone()));
    assert_eq!(current, original);
    let mut saved = original.clone();
    saved.full_mass_kg = Some(20.0);
    assert!(accept_calibration_save(&mut current, &original, saved.clone()));
    assert_eq!(current, saved);
}

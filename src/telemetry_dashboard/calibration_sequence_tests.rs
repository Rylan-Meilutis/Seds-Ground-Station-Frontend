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

/// Capture volts at known masses, then fit kg vs V. The fit is stored in
/// kg/V and the per-point residuals are reported in kg.
#[test]
fn volts_captured_at_known_masses_fit_kg_per_volt_with_residuals() {
    let mut cfg = CalibrationFile::default();
    // KG1000 legacy-raw zero 0.0122373 is about 0.2126 V; ~545.8 kg/V
    // (legacy 9481.6 kg/raw divided by 16000/921).
    let zero_v = 0.2126;
    let kg_per_v = 545.8_f32;
    apply_sequence_capture(&mut cfg, "ch1", CaptureMode::SequenceZero, 0.0, zero_v, true).unwrap();
    // One point is off by +0.02 kg so residuals are non-trivial.
    for (kg, offset_kg) in [(20.4_f32, 0.0_f32), (40.8, 0.02), (61.2, 0.0), (81.6, 0.0)] {
        let volts = zero_v + (kg + offset_kg) / kg_per_v;
        apply_sequence_capture(&mut cfg, "ch1", CaptureMode::SequencePoint, kg, volts, false).unwrap();
    }
    local_refit_channel(&mut cfg, "ch1", "linear").unwrap();
    let linear = &cfg.channels["ch1"].linear;
    assert!((linear.m.unwrap() - kg_per_v).abs() < 0.5, "slope {:?} kg/V", linear.m);
    let text = fit_details_text_voltage(&cfg, "ch1", calibration_capture::ReadingUnit::Volts).unwrap();
    assert!(text.starts_with("kg = m·V + b: slope 54") && text.contains(" kg/V") && text.contains("zero 0.212600 V"), "{text}");
    let residuals: Vec<f32> = channel_points_by_key(&cfg, "ch1")
        .iter()
        .map(|(volts, kg)| point_residual_kg(&cfg, "ch1", *volts, *kg).unwrap())
        .collect();
    // The offset point shows the largest residual, on the positive side.
    let worst = residuals.iter().copied().fold(0.0_f32, |a, b| if b.abs() > a.abs() { b } else { a });
    assert!(worst > 0.005 && worst < 0.03, "{residuals:?}");
    assert!((residuals[1] - worst).abs() < 1e-6, "{residuals:?}");
    // Span slope matches the linear slope.
    let readings: Vec<f32> = channel_points_by_key(&cfg, "ch1").iter().map(|(v, _)| *v).collect();
    assert!((fit_span_slope(&cfg, "ch1", &readings).unwrap() - linear.m.unwrap()).abs() < 0.05);
}

#[test]
fn polynomial_fit_summary_reports_mean_slope_in_kg_per_volt() {
    let mut cfg = CalibrationFile::default();
    let channel = cfg.channels.entry("kg50".into()).or_default();
    channel.zero_raw = Some(0.004);
    channel.fit = Some(FitMeta {
        fit_type: Some("poly2".into()),
        a: Some(0.0),
        b: Some(20_000.0),
        c: Some(0.0),
        ..Default::default()
    });
    channel.points = vec![CalibrationPoint { expected: 20.0, raw: 0.005 }];
    let text = fit_details_text_voltage(&cfg, "kg50", calibration_capture::ReadingUnit::Millivolts).unwrap();
    assert!(text.contains("poly2(V)") && text.contains("mean slope 20000.0") && text.contains(" kg/V over the points") && text.contains("zero 4.0000 mV"), "{text}");
}

#[test]
fn saves_echo_the_loaded_volts_version_and_legacy_payloads_stay_legacy() {
    assert_eq!(CalibrationFile::default().version, CALIBRATION_VERSION);
    assert_eq!(CALIBRATION_VERSION, 2);
    let loaded: CalibrationFile = serde_json::from_str(r#"{"version":2,"kg50_input":"amp2","full_mass_kg":10.0,"extra_channels":{"ch1":{"linear":{"m":545.8,"b":0.0},"zero_raw":0.2126,"points":[]}}}"#).unwrap();
    let request = CalibrationSaveRequest::new("ch1".into(), loaded);
    let body = serde_json::to_value(&request).unwrap();
    assert_eq!(body["calibration"]["version"], 2);
    // A payload without a version is a legacy raw-scale file; never relabel it.
    let legacy: CalibrationFile = serde_json::from_str(r#"{"full_mass_kg":10.0}"#).unwrap();
    assert_eq!(legacy.version, 1);
}

fn loaded_document() -> CalibrationFile {
    serde_json::from_str(r#"{"version":2,"kg50_input":"amp2","full_mass_kg":10.0,"revision":41,"mcp_vref_v":2.2,"mcp_vref_note":"measured","extra_channels":{"ch1":{"linear":{"m":545.8,"b":0.0},"zero_raw":0.2126,"points":[],"fitted_mcp_vref_v":2.2}}}"#).unwrap()
}

#[test]
fn a_save_names_the_revision_and_reference_the_draft_was_edited_from() {
    let mut draft = loaded_document();
    assert_eq!((draft.revision, draft.mcp_vref_v), (41, Some(2.2)));
    draft.channels.get_mut("ch1").unwrap().linear.m = Some(600.0);
    let body = serde_json::to_value(CalibrationSaveRequest::new("ch1".into(), draft)).unwrap();
    assert_eq!(body["channel"], "ch1");
    assert_eq!(body["revision"], 41);
    assert_eq!(body["fitted_mcp_vref_v"].as_f64().unwrap() as f32, 2.2);
    assert_eq!(body["calibration"]["extra_channels"]["ch1"]["linear"]["m"], 600.0);
    // A draft survives a reload of the page with the revision it came from.
    let draft = CalibrationDraft { calibration: loaded_document(), fit_mode: "best".into(), preserve_regression_on_zero_change: true };
    let restored: CalibrationDraft = serde_json::from_str(&serde_json::to_string(&draft).unwrap()).unwrap();
    assert_eq!((restored.calibration.revision, restored.calibration.mcp_vref_v), (41, Some(2.2)));
    // A document from before revisions is revision 0 with no reference; the
    // save then names none and the station decides (it refuses a load cell).
    let older: CalibrationFile = serde_json::from_str(r#"{"version":2,"full_mass_kg":10.0}"#).unwrap();
    let body = serde_json::to_value(CalibrationSaveRequest::new("ch1".into(), older)).unwrap();
    assert_eq!(body["revision"], 0);
    assert!(body.get("fitted_mcp_vref_v").is_none());
}

#[test]
fn a_conflict_is_stale_only_when_the_station_document_moved_on() {
    let submitted = loaded_document();
    let reason = "Calibration changed on the station since it was loaded (loaded revision 41, station revision 42). Nothing was saved: reload the calibration and re-apply your changes.";
    let error = format!("HTTP 409: {reason}");
    assert_eq!(http_error(&error), Some((409, reason)));
    assert_eq!(http_error("HTTP 503"), Some((503, "")));
    assert_eq!(http_error("Settings request timed out"), None);
    assert!(save_refusal_needs_station(&error));
    assert!(!save_refusal_needs_station("HTTP 503: Service Unavailable"));

    let mut moved = submitted.clone();
    moved.revision = 42;
    assert_eq!(save_refusal(&error, &submitted, Some(&moved)), SaveRefusal::Stale(reason.into()));
    let mut converted = submitted.clone();
    converted.mcp_vref_v = Some(2.4);
    assert!(matches!(save_refusal("HTTP 409: reference", &submitted, Some(&converted)), SaveRefusal::Stale(_)));
    let mut input = submitted.clone();
    input.kg50_input = "p6_differential".into();
    assert!(matches!(save_refusal("HTTP 409: input", &submitted, Some(&input)), SaveRefusal::Stale(_)));
    // A conflict about the edit itself (a railed point) keeps the draft saveable.
    let railed = "HTTP 409: KG1000 point 100 kg has 2.4 V at the ADC/amplifier rail";
    assert_eq!(save_refusal(railed, &submitted, Some(&submitted)), SaveRefusal::Failed(railed.into()));
    assert!(matches!(save_refusal(&error, &submitted, None), SaveRefusal::Failed(_)), "unverified: reported, not reloaded");
    assert!(matches!(save_refusal("HTTP 428: say which revision", &submitted, None), SaveRefusal::Stale(_)));
    assert!(matches!(save_refusal("HTTP 503: down", &submitted, Some(&moved)), SaveRefusal::Failed(_)));

    let message = stale_save_message(reason);
    assert!(message.starts_with("Not saved. Calibration changed on the station") && message.contains("reload"), "{message}");
    assert!(stale_save_message("  ").contains("changed since it was loaded"));
}

#[test]
fn newer_local_edits_move_to_the_revision_the_station_just_wrote() {
    let submitted = loaded_document();
    let mut current = submitted.clone();
    current.channels.get_mut("ch1").unwrap().zero_raw = Some(0.3);
    let mut saved = submitted.clone();
    saved.revision = 42;
    assert!(!accept_calibration_save(&mut current, &submitted, saved.clone()));
    assert_eq!(current.revision, 42, "edits made on top of the save are based on its revision");
    assert_eq!(current.channels["ch1"].zero_raw, Some(0.3));
    // A reply that skipped revisions means someone else wrote: stay stale.
    let mut current = submitted.clone();
    current.channels.get_mut("ch1").unwrap().zero_raw = Some(0.3);
    saved.revision = 44;
    assert!(!accept_calibration_save(&mut current, &submitted, saved.clone()));
    assert_eq!(current.revision, 41);
    // Nothing newer: the station's document replaces the draft.
    let mut current = submitted.clone();
    assert!(accept_calibration_save(&mut current, &submitted, saved.clone()));
    assert_eq!(current, saved);
}

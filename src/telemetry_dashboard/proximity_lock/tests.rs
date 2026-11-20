use super::*;
use serde_json::{Value, json};

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn haversine_vectors() {
    // Spherical (IUGG mean radius) haversine, the same function the watch
    // compass uses (watch/test-vectors.json "geodesy").
    for ((a, b), want, tol) in [
        (((0.0, 0.0), (0.0, 1.0)), 111_195.080, 0.01),
        (((42.0, -78.0), (42.001, -78.0)), 111.195, 0.001),
        (((42.0, -78.0), (42.0, -77.999)), 82.634, 0.001),
        (((32.99, -106.975), (32.9905, -106.9745)), 72.565, 0.001),
        (((0.0, 179.9995), (0.0, -179.9995)), 111.195, 0.001),
        (((51.5007, -0.1246), (40.6892, -74.0445)), 5_574_848.157, 1.0),
        (((42.0, -78.0), (42.0, -78.0)), 0.0, 1e-9),
    ] {
        let d = distance_m(LatLon::new(a.0, a.1), LatLon::new(b.0, b.1)).unwrap();
        assert!(close(d, want, tol), "{a:?} -> {b:?}: {d} vs {want}");
    }
    assert!(distance_m(LatLon::new(91.0, 0.0), LatLon::new(0.0, 0.0)).is_none());
    assert!(distance_m(LatLon::new(f64::NAN, 0.0), LatLon::new(0.0, 0.0)).is_none());
}

const FENCE: Geofence = Geofence { radius_m: 150.0, exit_margin_m: 30.0 };

#[test]
fn hysteresis_enters_at_the_radius_and_leaves_beyond_the_margin() {
    let acc = Some(0.0);
    // From out of range (or first fix): in at <= radius.
    for previous in [None, Some(false)] {
        assert!(FENCE.in_range(previous, 150.0, acc));
        assert!(FENCE.in_range(previous, 10.0, acc));
        assert!(!FENCE.in_range(previous, 150.1, acc));
        assert!(!FENCE.in_range(previous, 175.0, acc), "between radius and exit: stays out");
    }
    // From in range: stays in up to radius + margin.
    assert!(FENCE.in_range(Some(true), 175.0, acc));
    assert!(FENCE.in_range(Some(true), 180.0, acc));
    assert!(!FENCE.in_range(Some(true), 180.1, acc));
    // Walking out and back: no flapping in the band.
    let walk = [100.0, 149.0, 160.0, 170.0, 179.0, 170.0, 185.0, 175.0, 160.0, 150.0, 140.0];
    let expected = [true, true, true, true, true, true, false, false, false, true, true];
    let mut previous = None;
    for (d, want) in walk.iter().zip(expected) {
        let now = FENCE.in_range(previous, *d, acc);
        assert_eq!(now, want, "at {d} m from {previous:?}");
        previous = Some(now);
    }
}

#[test]
fn poor_accuracy_counts_as_in_range_when_its_circle_reaches_the_radius() {
    // 200 m away, accurate to 5 m: out. Accurate to 60 m: the circle reaches
    // 140 m, inside 150 m: in.
    assert!(!FENCE.in_range(None, 200.0, Some(5.0)));
    assert!(FENCE.in_range(None, 200.0, Some(60.0)));
    assert!(FENCE.in_range(None, 200.0, Some(50.0)), "exactly touching counts");
    assert!(!FENCE.in_range(None, 200.0, Some(49.9)));
    // Leaving needs the whole circle beyond radius + margin.
    assert!(FENCE.in_range(Some(true), 250.0, Some(70.0)));
    assert!(!FENCE.in_range(Some(true), 250.0, Some(69.0)));
    // An unknown or nonsense accuracy is taken as 100 m.
    assert!(FENCE.in_range(None, 250.0, None));
    assert!(!FENCE.in_range(None, 250.1, None));
    assert!(FENCE.in_range(None, 250.0, Some(f64::NAN)));
    assert!(FENCE.in_range(None, 250.0, Some(-1.0)));
    assert_eq!(accuracy_class(Some(5.0)), Accuracy::High);
    assert_eq!(accuracy_class(Some(20.0)), Accuracy::High);
    assert_eq!(accuracy_class(Some(65.0)), Accuracy::Medium);
    assert_eq!(accuracy_class(Some(100.1)), Accuracy::Low);
    assert_eq!(accuracy_class(None), Accuracy::Low);
}

#[test]
fn the_reference_is_a_fresh_rocket_fix_else_the_pad_else_nothing() {
    let now = 1_000_000_000;
    let pad = Some(Pad { lat_deg: 42.0, lon_deg: -78.0 });
    let rocket = Some(((42.0005, -78.0), now - 1_000));
    assert_eq!(choose_reference(rocket, pad, now), ReferencePoint::Rocket(LatLon::new(42.0005, -78.0)));
    let stale = Some(((42.0005, -78.0), now - ROCKET_FIX_FRESH_MS - 1));
    assert_eq!(choose_reference(stale, pad, now), ReferencePoint::Pad(LatLon::new(42.0, -78.0)));
    assert_eq!(choose_reference(stale, None, now), ReferencePoint::None);
    let null_island = Some(((0.0, 0.0), now));
    assert_eq!(choose_reference(null_island, pad, now).kind(), Some(Reference::Pad));
    let future = Some(((42.0005, -78.0), now + 60_000));
    assert_eq!(choose_reference(future, None, now), ReferencePoint::None, "a clock far ahead is not fresh");
    assert_eq!(choose_reference(None, Some(Pad { lat_deg: 95.0, lon_deg: 0.0 }), now), ReferencePoint::None);
}

#[test]
fn evaluation_needs_a_fresh_fix_and_a_reference() {
    let now = 2_000_000_000;
    let reference = ReferencePoint::Rocket(LatLon::new(42.0, -78.0));
    let near = OwnFix { lat: 42.001, lon: -78.0, accuracy_m: Some(5.0), at_ms: now - 2_000 };
    let far = OwnFix { lat: 42.01, lon: -78.0, accuracy_m: Some(5.0), at_ms: now - 2_000 };
    assert_eq!(evaluate(&FENCE, None, Some(near), reference, now), Ok(true));
    assert_eq!(evaluate(&FENCE, None, Some(far), reference, now), Ok(false));
    assert_eq!(evaluate(&FENCE, None, None, reference, now), Err(Unknown::NoFix));
    let old = OwnFix { at_ms: now - OWN_FIX_FRESH_MS - 1, ..near };
    assert_eq!(evaluate(&FENCE, None, Some(old), reference, now), Err(Unknown::NoFix));
    assert_eq!(evaluate(&FENCE, None, Some(near), ReferencePoint::None, now), Err(Unknown::NoReference));
}

#[test]
fn participation_follows_station_sign_in_opt_out_and_permission() {
    use Participation as P;
    use Permission as Pm;
    assert_eq!(P::of(false, true, false, Pm::Background), P::StationOff);
    assert_eq!(P::of(true, false, false, Pm::Background), P::SignedOut);
    assert_eq!(P::of(true, true, true, Pm::Background), P::OptedOut);
    assert_eq!(P::of(true, true, false, Pm::Denied), P::PermissionDenied);
    assert_eq!(P::of(true, true, false, Pm::Unsupported), P::Unsupported);
    assert_eq!(P::of(true, true, false, Pm::NotDetermined), P::AwaitingPermission);
    assert_eq!(P::of(true, true, false, Pm::Foreground), P::Participating);
    assert_eq!(P::of(true, true, false, Pm::Background), P::Participating);
    // Opting out wins over any permission; denial counts as opting out.
    assert_eq!(P::of(true, true, true, Pm::Denied).opt_out(), Some(OptOut::User));
    assert_eq!(P::PermissionDenied.opt_out(), Some(OptOut::PermissionDenied));
    assert!(!P::StationOff.reports() && !P::SignedOut.reports());
    assert!(P::OptedOut.reports() && !P::OptedOut.participating());
    assert!(P::AwaitingPermission.participating());
}

fn sample_report(participation: Participation, result: Option<Result<bool, Unknown>>) -> Report {
    let fix = OwnFix { lat: 42.123456, lon: -78.654321, accuracy_m: Some(7.5), at_ms: 1_700_000_000_000 };
    build_report(
        "prox-1",
        "Alex's iPhone",
        Platform::Ios,
        participation,
        result,
        Some(fix),
        ReferencePoint::Rocket(LatLon::new(42.124, -78.655)),
        true,
        7,
        1_700_000_000_500,
    )
}

#[test]
fn the_serialized_report_carries_no_coordinates_distance_or_bearing() {
    let report = sample_report(Participation::Participating, Some(Ok(true)));
    let text = serde_json::to_string(&report).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    let keys: std::collections::BTreeSet<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "accuracy", "background", "device_id", "device_name", "in_range", "opt_out", "participating",
            "platform", "reference", "seq", "sent_ms", "v",
        ]
        .into()
    );
    for key in &keys {
        for word in ["lat", "lon", "dist", "bearing", "coord", "position", "location", "heading"] {
            assert!(*key == "platform" || !key.contains(word), "{key} looks like a place");
        }
        assert!(!key.ends_with("_m") && !key.ends_with("_deg"), "{key} looks like a measurement");
    }
    for word in ["latitude", "longitude", "distance", "bearing", "coord"] {
        assert!(!text.to_lowercase().contains(word), "{word} in {text}");
    }
    // Neither the device's nor the rocket's coordinates appear in any form.
    for number in ["42.12", "78.65", "42.124", "78.655", "7.5", "123456", "654321"] {
        assert!(!text.contains(number), "{number} in {text}");
    }
    // Every numeric field is a counter, a version or a time.
    for (key, v) in value.as_object().unwrap() {
        if v.is_number() {
            assert!(matches!(key.as_str(), "v" | "seq" | "sent_ms"), "{key}");
        }
        if let Some(f) = v.as_f64() {
            assert_eq!(f.fract(), 0.0, "{key} is not an integer");
        }
    }
    assert_eq!(
        value,
        json!({
            "v": 1, "device_id": "prox-1", "device_name": "Alex's iPhone", "platform": "ios",
            "participating": true, "in_range": true, "accuracy": "high", "reference": "rocket",
            "background": true, "opt_out": null, "seq": 7, "sent_ms": 1_700_000_000_500i64,
        })
    );
}

#[test]
fn a_non_participating_report_says_only_why() {
    for (participation, why) in [
        (Participation::OptedOut, "user"),
        (Participation::PermissionDenied, "permission_denied"),
        (Participation::Unsupported, "unsupported"),
    ] {
        let value = serde_json::to_value(sample_report(participation, Some(Ok(true)))).unwrap();
        assert_eq!(value["participating"], false);
        assert_eq!(value["opt_out"], why);
        for empty in ["in_range", "accuracy", "reference"] {
            assert!(value[empty].is_null(), "{empty} with {participation:?}");
        }
        assert_eq!(value["background"], false);
    }
    // Participating but cannot tell: null, which the station treats as unknown.
    let value = serde_json::to_value(sample_report(Participation::AwaitingPermission, Some(Err(Unknown::NoFix)))).unwrap();
    assert_eq!((value["participating"].as_bool(), value["in_range"].is_null()), (Some(true), true));
}

#[test]
fn reports_go_out_on_change_and_on_the_heartbeat() {
    let first = sample_report(Participation::Participating, Some(Ok(false)));
    assert!(report_due(None, &first, 0));
    let mut again = first.clone();
    again.seq += 1;
    again.sent_ms += 1;
    assert!(!report_due(Some((&first, 1_000)), &again, 2_000));
    assert!(report_due(Some((&first, 1_000)), &again, 1_000 + HEARTBEAT_MS));
    let entered = sample_report(Participation::Participating, Some(Ok(true)));
    assert!(report_due(Some((&first, 1_000)), &entered, 1_001), "a change is sent at once");
    let out = sample_report(Participation::OptedOut, None);
    assert!(report_due(Some((&first, 1_000)), &out, 1_001));
}

#[test]
fn status_decodes_the_station_contract() {
    let status: Status = serde_json::from_value(json!({
        "settings": {"enabled": true, "radius_m": 150, "exit_margin_m": 30,
            "blocks": {"arming": true, "fill": true, "launch": false}, "stale_policy": "block", "heartbeat_timeout_s": 60},
        "heartbeat_interval_s": 15, "active": true, "condition": true,
        "reason": "Proximity lock: 1 person within 150 m of the rocket (Alex's iPhone)",
        "summary": "Proximity lock: 4 devices reporting, 1 in range (Alex's iPhone), 2 opted out",
        "counts": {"reporting": 4, "in_range": 1, "unknown": 0, "opted_out": 2},
        "devices": [{"id": "abc", "name": "Alex's iPhone", "user": "alex", "platform": "ios", "state": "in_range",
            "accuracy": "high", "reference": "rocket", "background": true, "last_report_ms": 5}],
        "pad": {"lat_deg": 42.0, "lon_deg": -78.0},
        "blocked_commands": ["StartFill", "Launch"],
        "override": {"by": "admin", "reason": "pad crew", "expires_ms": 99, "remaining_s": 60},
    }))
    .unwrap();
    assert_eq!(status.settings.stale_policy, StalePolicy::Block);
    assert!(!status.settings.blocks.launch);
    assert_eq!(status.names(DeviceState::InRange), vec!["Alex's iPhone".to_string()]);
    assert_eq!(status.override_view.as_ref().unwrap().by, "admin");
    assert_eq!(serde_json::from_value::<Status>(json!({})).unwrap(), Status::default());
}

#[test]
fn banners_by_severity() {
    let mut status = Status { settings: Settings { enabled: true, ..Settings::default() }, summary: "Proximity lock: 2 devices reporting, 0 opted out".into(), ..Status::default() };
    assert_eq!(banner(None), None);
    assert_eq!(banner(Some(&Status::default())), None, "off: nothing shown");
    assert_eq!(banner(Some(&status)), Some(Banner::Indicator(status.summary.clone())));
    status.warning = Some("unknown".into());
    assert_eq!(banner(Some(&status)), Some(Banner::Warning("unknown".into())));
    status.condition = true;
    status.reason = Some("Proximity lock: 1 person".into());
    status.override_view = Some(OverrideView { by: "admin".into(), reason: "crew".into(), expires_ms: 120_000, remaining_s: 0 });
    assert!(matches!(banner(Some(&status)), Some(Banner::Overridden { .. })));
    status.active = true;
    assert_eq!(banner(Some(&status)), Some(Banner::Active("Proximity lock: 1 person".into())));
    assert_eq!(remaining_text(125_000, 0), "2:05");
    assert_eq!(remaining_text(0, 10), "0:00");
}

#[test]
fn settings_and_override_requests() {
    let current = Settings::default();
    let next = Settings { enabled: true, radius_m: 300, ..current };
    let update = SettingsUpdate::between(&current, &next);
    assert_eq!(serde_json::to_value(&update).unwrap(), json!({"enabled": true, "radius_m": 300}));
    assert!(SettingsUpdate::between(&current, &current).is_empty());
    assert!(validate_settings(&next).is_ok());
    for bad in [
        Settings { radius_m: 24, ..next },
        Settings { radius_m: 2001, ..next },
        Settings { exit_margin_m: 4, ..next },
        Settings { heartbeat_timeout_s: 29, ..next },
        Settings { blocks: Blocks { arming: false, fill: false, launch: false, manual_valves: false }, ..next },
    ] {
        assert!(validate_settings(&bad).is_err(), "{bad:?}");
    }
    assert_eq!(
        serde_json::to_value(validate_override(10, " pad crew filling ").unwrap()).unwrap(),
        json!({"minutes": 10, "reason": "pad crew filling", "confirm": true})
    );
    assert!(validate_override(0, "pad crew").is_err());
    assert!(validate_override(31, "pad crew").is_err());
    assert!(validate_override(5, "ok").is_err());
}

#[test]
fn local_preferences_and_names() {
    let prefs = Prefs::parse(r#"{"accounts":{"https://gs|alex":{"opted_out":true,"device_name":"Field phone"}}}"#);
    assert!(prefs.account("https://gs|alex").opted_out);
    assert_eq!(prefs.account("https://gs|alex").device_name.as_deref(), Some("Field phone"));
    assert!(!prefs.account("https://gs|bo").opted_out, "participating by default");
    assert_eq!(Prefs::parse("garbage"), Prefs::default());
    assert_eq!(default_device_name("alex", Platform::Ios), "alex's iPhone");
    assert_eq!(default_device_name("bo", Platform::Web), "bo's browser");
    assert!(valid_device_name("Alex's iPhone"));
    assert!(!valid_device_name("   "));
    assert!(!valid_device_name(&"x".repeat(49)));
    assert!(!valid_device_name("a\nb"));
    assert_eq!(new_device_id(255), "prox-000000000000000000000000000000ff");
    assert!(background_capable(Platform::Android, Permission::Background));
    assert!(!background_capable(Platform::Android, Permission::Foreground));
    assert!(!background_capable(Platform::Web, Permission::Background));
}

#[test]
fn own_state_lines() {
    let rocket = ReferencePoint::Rocket(LatLon::new(1.0, 1.0));
    let pad = ReferencePoint::Pad(LatLon::new(1.0, 1.0));
    assert!(own_state_text(Participation::Participating, Some(Ok(true)), rocket).starts_with("In range (measured against the rocket"));
    assert!(own_state_text(Participation::Participating, Some(Ok(false)), pad).contains("configured launch site"));
    assert!(own_state_text(Participation::PermissionDenied, None, rocket).contains("counts as opting out"));
    assert!(own_state_text(Participation::OptedOut, None, rocket).starts_with("Not participating"));
    assert!(own_state_text(Participation::Participating, Some(Err(Unknown::NoReference)), ReferencePoint::None).contains("no launch site"));
}

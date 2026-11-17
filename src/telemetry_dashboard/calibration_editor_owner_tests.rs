use super::*;

#[test]
fn actual_callback_rejects_old_input_and_editor_lifetimes() {
    let mut dom = VirtualDom::new(|| rsx! { div {} });
    dom.rebuild_in_place();
    dom.in_scope(ScopeId::ROOT, || {
        let mut input_epoch = Signal::new(0_u64);
        let mut revision = Signal::new(0_u64);
        let editor = CalibrationEditorOwner {
            input_epoch,
            revision,
            retired: Signal::new(None),
        };
        let callback = EventHandler::new(move |original| {
            editor.retire(original);
        });
        let first = editor.token();
        callback.call(first);
        assert!(editor.is_retired());
        assert!(editor.reload());
        assert!(!editor.is_retired());
        callback.call(first);
        assert!(
            !editor.is_retired(),
            "late old callback cannot retire new editor"
        );
        let second = editor.token();
        callback.call(second);
        assert!(editor.is_retired());
        assert!(editor.invalidate());
        assert!(
            editor.is_retired(),
            "unmount cleanup alone cannot acknowledge a retired read"
        );
        let same_input = editor.token();
        input_epoch.set(1);
        assert!(
            !editor.is_retired(),
            "a new actual input lifetime cannot inherit old read retirement"
        );
        callback.call(same_input);
        assert!(
            !editor.is_retired(),
            "old input callback cannot retire new source"
        );
        callback.call(editor.token());
        assert!(editor.is_retired());
        revision.set(u64::MAX - 1);
        assert!(!editor.reload());
        assert_eq!(*editor.revision.peek(), u64::MAX);
    });
}

#[test]
fn explicit_read_retry_never_acknowledges_pending_or_uncertain_input_writes() {
    let mut dom = VirtualDom::new(|| rsx! { div {} });
    dom.rebuild_in_place();
    dom.in_scope(ScopeId::ROOT, || {
        let mut selection = Signal::new(InputSelectionStatus::Ready);
        let context = CalibrationContext {
            input_epoch: Signal::new(9),
            selection,
            locked: Signal::new(false),
            request: EventHandler::new(|_: String| {}),
            retained_owner: Signal::new(None),
            abort_only: Signal::new(false),
        };
        for state in [
            InputSelectionStatus::Pending("amp2".into()),
            InputSelectionStatus::Unconfirmed {
                requested: "amp2".into(),
                detail: "unknown".into(),
            },
            InputSelectionStatus::Retired("write owner retired".into()),
        ] {
            selection.set(state.clone());
            assert!(!context.prepare_read_retry());
            assert_eq!(*selection.peek(), state);
            assert_eq!(*context.input_epoch.peek(), 9);
        }
        selection.set(InputSelectionStatus::Confirmed("p6_differential".into()));
        assert!(context.prepare_read_retry());
        assert_eq!(*selection.peek(), InputSelectionStatus::Ready);
        assert_eq!(
            *context.input_epoch.peek(),
            9,
            "read retry cannot restamp input ownership"
        );
    });
}

fn readback(selected: &str, state: &str) -> Kg50BoardStatus {
    Kg50BoardStatus {
        selected: selected.into(),
        board_input: Some(selected.into()),
        board_age_ms: Some(100),
        state: state.into(),
    }
}

#[test]
fn refused_input_write_stays_retryable_for_either_input() {
    let refused = InputSelectionStatus::Unconfirmed {
        requested: "amp2".into(),
        detail: "HTTP 409: Recovery admission is closed or request is stale".into(),
    };
    assert!(refused.accepts_request(), "the same or the other input can be sent again");
    assert!(!InputSelectionStatus::Pending("amp2".into()).accepts_request());
    assert!(InputSelectionStatus::Confirmed("amp2".into()).accepts_request());
}

#[test]
fn synced_station_readback_settles_an_unconfirmed_write() {
    let refused = InputSelectionStatus::Unconfirmed {
        requested: "amp2".into(),
        detail: "HTTP 409".into(),
    };
    assert_eq!(
        refused.resolved_by_readback(&readback("p6_differential", "synced")),
        Some(InputSelectionStatus::Confirmed("p6_differential".into()))
    );
    assert!(!refused
        .resolved_by_readback(&readback("p6_differential", "synced"))
        .unwrap()
        .captures_blocked());
    assert_eq!(refused.resolved_by_readback(&readback("amp2", "syncing")), None);
    assert_eq!(refused.resolved_by_readback(&readback("amp2", "mismatch")), None);
    assert_eq!(refused.resolved_by_readback(&readback("bogus", "synced")), None);
    // Only an unconfirmed write is settled this way; nothing replaces a
    // write in flight or a retired owner's state.
    assert_eq!(InputSelectionStatus::Pending("amp2".into()).resolved_by_readback(&readback("amp2", "synced")), None);
    assert_eq!(InputSelectionStatus::Retired("x".into()).resolved_by_readback(&readback("amp2", "synced")), None);
}

#[test]
fn readback_settles_only_in_the_epoch_it_was_taken() {
    let mut dom = VirtualDom::new(|| rsx! { div {} });
    dom.rebuild_in_place();
    dom.in_scope(ScopeId::ROOT, || {
        let context = CalibrationContext {
            input_epoch: Signal::new(4),
            selection: Signal::new(InputSelectionStatus::Unconfirmed { requested: "amp2".into(), detail: "HTTP 409".into() }),
            locked: Signal::new(false),
            request: EventHandler::new(|_: String| {}),
            retained_owner: Signal::new(None),
            abort_only: Signal::new(false),
        };
        assert_eq!(context.settle_from_readback(3, &readback("amp2", "synced")), None, "older epoch");
        assert!(context.captures_blocked());
        assert_eq!(context.settle_from_readback(4, &readback("amp2", "synced")), Some("amp2".into()));
        assert!(!context.captures_blocked());
    });
}

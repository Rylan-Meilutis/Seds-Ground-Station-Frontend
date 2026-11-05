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

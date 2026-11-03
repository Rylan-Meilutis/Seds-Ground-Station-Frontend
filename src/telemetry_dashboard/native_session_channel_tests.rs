//! Actual native channel/select controls; no socket or board commands are sent.
use super::native_session::{Owner, await_owned};
use std::{cell::RefCell, future::Future, rc::Rc, task::Poll};

#[derive(Clone)]
struct Current {
    alive: bool,
    epoch: u64,
    station: String,
    token: Option<String>,
}
fn fixture() -> (Owner, Rc<RefCell<Current>>) {
    (
        Owner::new(7, "wss://original".into(), Some("test-token".into())),
        Rc::new(RefCell::new(Current {
            alive: true,
            epoch: 7,
            station: "wss://original".into(),
            token: Some("test-token".into()),
        })),
    )
}
fn current(owner: &Owner, state: &RefCell<Current>) -> bool {
    let state = state.borrow();
    owner.is_current(
        state.alive,
        state.epoch,
        &state.station,
        state.token.as_deref(),
    )
}
async fn hold_pending<F: Future>(mut future: std::pin::Pin<&mut F>) {
    std::future::poll_fn(|context| {
        assert!(future.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
}

#[tokio::test]
async fn held_actual_select_rejects_old_incoming_and_queued_outgoing_after_each_owner_switch() {
    for switch in ["station", "auth", "epoch", "mount"] {
        for outgoing in [false, true] {
            let (owner, state) = fixture();
            let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let (in_tx, mut in_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let selected = async {
                tokio::select! { event=out_rx.recv()=>event, event=in_rx.recv()=>event }
            };
            let guarded = await_owned(selected, || current(&owner, &state));
            let mut guarded = std::pin::pin!(guarded);
            hold_pending(guarded.as_mut()).await;
            match switch {
                "station" => state.borrow_mut().station = "wss://replacement".into(),
                "auth" => state.borrow_mut().token = None,
                "epoch" => state.borrow_mut().epoch += 1,
                _ => state.borrow_mut().alive = false,
            }
            if outgoing {
                out_tx.send("queued-command".into()).unwrap();
            } else {
                in_tx.send("old-frame".into()).unwrap();
            }
            assert_eq!(guarded.await, None, "{switch}, outgoing={outgoing}");
        }
    }
}

#[tokio::test]
async fn causal_original_preawait_only_select_dispatches_retired_actual_channel_event() {
    for outgoing in [false, true] {
        let (owner, state) = fixture();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (in_tx, mut in_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let original = async {
            if !current(&owner, &state) {
                return None;
            }
            tokio::select! {event=out_rx.recv()=>event,event=in_rx.recv()=>event}
        };
        let mut original = std::pin::pin!(original);
        hold_pending(original.as_mut()).await;
        state.borrow_mut().epoch += 1;
        if outgoing {
            out_tx.send("queued-command".into()).unwrap();
        } else {
            in_tx.send("old-frame".into()).unwrap();
        }
        assert!(
            original.await.is_some(),
            "unchanged original loop reproduces stale incoming/outgoing effect"
        );
    }
}

#[tokio::test]
async fn current_native_select_delivers_both_directions_without_changes() {
    for outgoing in [false, true] {
        let (owner, state) = fixture();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (in_tx, mut in_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let selected = async {
            tokio::select! {event=out_rx.recv()=>event,event=in_rx.recv()=>event}
        };
        let guarded = await_owned(selected, || current(&owner, &state));
        let mut guarded = std::pin::pin!(guarded);
        hold_pending(guarded.as_mut()).await;
        if outgoing {
            out_tx.send("current".into()).unwrap();
        } else {
            in_tx.send("current".into()).unwrap();
        }
        assert_eq!(guarded.await, Some(Some("current".into())));
    }
}

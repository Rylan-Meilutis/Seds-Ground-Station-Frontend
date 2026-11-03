//! Own an asynchronous native transport result across every suspension point.
//! Cancellation cannot recall bytes accepted by the socket before invalidation.
use std::{future::Future, task::Poll};

pub(super) struct Owner {
    epoch: u64,
    base_ws: String,
    token: Option<String>,
}
impl Owner {
    pub(super) fn new(epoch: u64, base_ws: String, token: Option<String>) -> Self {
        Self {
            epoch,
            base_ws,
            token,
        }
    }
    pub(super) fn is_current(
        &self,
        alive: bool,
        epoch: u64,
        base_ws: &str,
        token: Option<&str>,
    ) -> bool {
        alive && self.epoch == epoch && self.base_ws == base_ws && self.token.as_deref() == token
    }
}

pub(super) async fn await_owned<F: Future>(
    future: F,
    is_current: impl Fn() -> bool,
) -> Option<F::Output> {
    let mut future = std::pin::pin!(future);
    std::future::poll_fn(move |context| {
        if !is_current() {
            return Poll::Ready(None);
        }
        match future.as_mut().poll(context) {
            Poll::Ready(value) if is_current() => Poll::Ready(Some(value)),
            Poll::Ready(_) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        pin::Pin,
        rc::Rc,
        task::{Context, Waker},
    };
    fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }
    struct Held {
        ready: Rc<Cell<bool>>,
        polls: Rc<Cell<u32>>,
        invalidate_on_ready: Option<Rc<Cell<bool>>>,
    }
    impl Future for Held {
        type Output = &'static str;
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            self.polls.set(self.polls.get() + 1);
            if !self.ready.get() {
                return Poll::Pending;
            }
            if let Some(owner) = &self.invalidate_on_ready {
                owner.set(false);
            }
            Poll::Ready("original-session-event")
        }
    }
    fn held() -> (Held, Rc<Cell<bool>>, Rc<Cell<u32>>) {
        let ready = Rc::new(Cell::new(false));
        let polls = Rc::new(Cell::new(0));
        (
            Held {
                ready: ready.clone(),
                polls: polls.clone(),
                invalidate_on_ready: None,
            },
            ready,
            polls,
        )
    }
    #[test]
    fn original_owner_checks_station_authentication_epoch_and_mount_independently() {
        let owner = Owner::new(9, "wss://original".into(), Some("original-token".into()));
        assert!(owner.is_current(true, 9, "wss://original", Some("original-token")));
        assert!(!owner.is_current(true, 9, "wss://next", Some("original-token")));
        assert!(!owner.is_current(true, 9, "wss://original", Some("next-token")));
        assert!(!owner.is_current(true, 10, "wss://original", Some("original-token")));
        assert!(!owner.is_current(false, 9, "wss://original", Some("original-token")));
        assert!(!owner.is_current(true, 9, "wss://original", None));
    }
    // Causal negative: the original production loop checks only before waiting.
    #[test]
    fn original_preawait_only_control_dispatches_after_station_switch() {
        for direction in ["incoming", "outgoing"] {
            let current = Rc::new(Cell::new(true));
            let (future, ready, _) = held();
            let observed = current.clone();
            let old_loop = async move {
                if observed.get() {
                    Some(future.await)
                } else {
                    None
                }
            };
            let mut old_loop = std::pin::pin!(old_loop);
            assert!(poll(old_loop.as_mut()).is_pending());
            current.set(false);
            ready.set(true);
            assert!(
                matches!(poll(old_loop.as_mut()), Poll::Ready(Some(_))),
                "original {direction} path reproduces stale effect"
            );
        }
    }
    #[test]
    fn held_incoming_and_outgoing_events_are_retired_before_effects() {
        for switch in ["station", "authentication", "epoch", "unmount"] {
            for direction in ["incoming", "outgoing", "connect", "pong"] {
                let current = Rc::new(Cell::new(true));
                let (future, ready, polls) = held();
                let mut guarded = std::pin::pin!(await_owned(future, || current.get()));
                assert!(poll(guarded.as_mut()).is_pending());
                assert_eq!(polls.get(), 1);
                current.set(false);
                ready.set(true);
                assert_eq!(
                    poll(guarded.as_mut()),
                    Poll::Ready(None),
                    "{switch} {direction}"
                );
                assert_eq!(
                    polls.get(),
                    1,
                    "retired send/receive must not be polled again"
                );
            }
        }
    }
    #[test]
    fn current_session_keeps_identical_event_and_send_completion() {
        let (future, ready, polls) = held();
        let mut guarded = std::pin::pin!(await_owned(future, || true));
        assert!(poll(guarded.as_mut()).is_pending());
        ready.set(true);
        assert_eq!(
            poll(guarded.as_mut()),
            Poll::Ready(Some("original-session-event"))
        );
        assert_eq!(polls.get(), 2);
    }
    #[test]
    fn invalidation_during_ready_poll_cannot_publish_result() {
        let current = Rc::new(Cell::new(true));
        let (mut future, ready, _) = held();
        future.invalidate_on_ready = Some(current.clone());
        ready.set(true);
        let mut guarded = std::pin::pin!(await_owned(future, || current.get()));
        assert_eq!(poll(guarded.as_mut()), Poll::Ready(None));
    }
    #[test]
    fn already_retired_connection_never_polls_transport() {
        let (future, _, polls) = held();
        let mut guarded = std::pin::pin!(await_owned(future, || false));
        assert_eq!(poll(guarded.as_mut()), Poll::Ready(None));
        assert_eq!(polls.get(), 0);
    }
}

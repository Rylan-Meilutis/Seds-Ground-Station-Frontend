//! Test-only scheduler barrier; no discovery data, socket or timeout overrides.
use std::{sync::{Arc, Weak, Mutex, Condvar, mpsc}, time::{Duration, Instant}};

static ACTIVE: Mutex<Option<Weak<Gate>>> = Mutex::new(None);
struct Gate {
    released: Mutex<bool>,
    changed: Condvar,
    entered: mpsc::Sender<()>,
    completed: mpsc::Sender<bool>,
}
/// Releasing on Drop prevents an assertion from leaving the real worker held.
pub(crate) struct Probe {
    gate: Arc<Gate>,
    pub(crate) entered: mpsc::Receiver<()>,
    pub(crate) completed: mpsc::Receiver<bool>,
}
impl Probe {
    pub(crate) fn install() -> Self {
        let (entered, receiver) = mpsc::channel();
        let (completed, finished) = mpsc::channel();
        let gate = Arc::new(Gate { released: Mutex::new(false), changed: Condvar::new(), entered, completed });
        let mut active = ACTIVE.lock().unwrap();
        assert!(active.as_ref().and_then(Weak::upgrade).is_none(), "one barrier per isolated test");
        *active = Some(Arc::downgrade(&gate));
        Self { gate, entered: receiver, completed: finished }
    }
    pub(crate) fn release(&self) {
        *self.gate.released.lock().unwrap() = true;
        self.gate.changed.notify_all();
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.release();
        let mut active = ACTIVE.lock().unwrap();
        if active.as_ref().and_then(Weak::upgrade).is_some_and(|old| Arc::ptr_eq(&old, &self.gate)) {
            active.take();
        }
    }
}
/// Completion means the original body returned, including a real Err. It is
/// not discovery success, an OS pool thread join or a transport safety proof.
pub(crate) struct Attempt { gate: Arc<Gate> }
impl Drop for Attempt {
    fn drop(&mut self) { let _ = self.gate.completed.send(!std::thread::panicking()); }
}
pub(crate) fn enter_actual_worker() -> Option<Attempt> {
    let gate = ACTIVE.lock().unwrap().as_ref().and_then(Weak::upgrade)?;
    let _ = gate.entered.send(());
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut released = gate.released.lock().unwrap();
    while !*released {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { drop(released); panic!("actual worker barrier release exceeded test deadline"); }
        released = gate.changed.wait_timeout(released, remaining).unwrap().0;
    }
    drop(released);
    Some(Attempt { gate })
}

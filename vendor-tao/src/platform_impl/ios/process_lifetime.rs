//! Main-thread TLS can lend a state guard across UIKit callback transitions.
//! Keep one allocation for the process lifetime, matching the previous static.
//! This holder is deliberately not Sync; callers retain their main-thread check.
use std::cell::{OnceCell, RefCell};

pub(super) struct ProcessLifetime<T: 'static> {
  cell: OnceCell<&'static RefCell<T>>,
}

impl<T: 'static> ProcessLifetime<T> {
  pub(super) const fn new() -> Self {
    Self {
      cell: OnceCell::new(),
    }
  }

  pub(super) fn get_or_init(&self, init: impl FnOnce() -> T) -> &'static RefCell<T> {
    self
      .cell
      .get_or_init(|| Box::leak(Box::new(RefCell::new(init()))))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::cell::Cell;

  #[test]
  fn callback_borrows_remain_exclusive_and_initialize_once() {
    let holder = ProcessLifetime::new();
    let calls = Cell::new(0);
    let cell = holder.get_or_init(|| {
      calls.set(calls.get() + 1);
      Vec::<u8>::new()
    });
    let mut callback = cell.borrow_mut();
    callback.push(7);
    let same = holder.get_or_init(|| panic!("state cannot reinitialize during callback"));
    assert!(std::ptr::eq(cell, same));
    assert!(same.try_borrow_mut().is_err());
    drop(callback);
    same.borrow_mut().push(9);
    assert_eq!(*cell.borrow(), [7, 9]);
    assert_eq!(calls.get(), 1);
  }

  #[test]
  fn process_lifetime_guard_does_not_borrow_the_tls_holder() {
    let holder = ProcessLifetime::new();
    let cell = holder.get_or_init(|| 11);
    let mut guard = cell.borrow_mut();
    drop(holder);
    *guard = 12;
    drop(guard);
    assert_eq!(*cell.borrow(), 12);
  }

  #[test]
  fn thread_local_holders_have_independent_borrow_state() {
    thread_local! {
      static STATE: ProcessLifetime<usize> = const { ProcessLifetime::new() };
    }
    let local = STATE.with(|holder| holder.get_or_init(|| 1));
    let mut held = local.borrow_mut();
    let worker = std::thread::spawn(|| {
      STATE.with(|holder| {
        let state = holder.get_or_init(|| 2);
        *state.borrow_mut() = 3;
        *state.borrow()
      })
    });
    assert_eq!(worker.join().unwrap(), 3);
    *held = 4;
    drop(held);
    assert_eq!(*local.borrow(), 4);
  }
}

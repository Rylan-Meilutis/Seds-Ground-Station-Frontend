// Copyright 2026 GroundStation contributors
// SPDX-License-Identifier: Apache-2.0

use gtk::glib;

/// Run GTK callbacks on their owning context without a deprecated GLib channel.
/// Producers use an unbounded channel's nonblocking try_send, just as before.
pub(super) fn attach<T: 'static>(
  context: &glib::MainContext,
  receiver: async_channel::Receiver<T>,
  mut callback: impl FnMut(T) -> glib::ControlFlow + 'static,
) -> glib::JoinHandle<()> {
  context.spawn_local(async move {
    let mut batch = 0;
    while let Ok(event) = receiver.recv().await {
      if callback(event) == glib::ControlFlow::Break {
        break;
      }
      batch += 1;
      if batch == 64 {
        // A busy sender must not prevent redraws/input from being dispatched.
        glib::timeout_future_with_priority(glib::Priority::DEFAULT_IDLE, std::time::Duration::ZERO)
          .await;
        batch = 0;
      }
    }
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{
    cell::{Cell, RefCell},
    rc::Rc,
  };

  #[test]
  fn fifo_burst_drains_before_sender_closure_completes() {
    let context = glib::MainContext::new();
    context
      .with_thread_default(|| {
        let (sender, receiver) = async_channel::unbounded();
        for value in 0..4096 {
          sender.try_send(value).unwrap();
        }
        drop(sender);
        let received = Rc::new(RefCell::new(Vec::new()));
        let output = received.clone();
        let task = attach(&context, receiver, move |value| {
          output.borrow_mut().push(value);
          glib::ControlFlow::Continue
        });
        context.block_on(task).unwrap();
        assert_eq!(*received.borrow(), (0..4096).collect::<Vec<_>>());
      })
      .unwrap();
  }

  #[test]
  fn worker_wakes_the_main_context_and_callbacks_stay_on_its_thread() {
    let context = glib::MainContext::new();
    context
      .with_thread_default(|| {
        let main_thread = std::thread::current().id();
        let (sender, receiver) = async_channel::unbounded();
        let delivered = Rc::new(Cell::new(false));
        let result = delivered.clone();
        let task = attach(&context, receiver, move |value| {
          assert_eq!(std::thread::current().id(), main_thread);
          assert_eq!(value, 42);
          result.set(true);
          glib::ControlFlow::Continue
        });
        // Poll until the consumer is genuinely pending before a different thread sends.
        context.iteration(false);
        let worker = std::thread::spawn(move || sender.try_send(42).unwrap());
        context.block_on(task).unwrap();
        worker.join().unwrap();
        assert!(delivered.get());
      })
      .unwrap();
  }

  #[test]
  fn callback_break_discards_remaining_events_and_closes_the_producer() {
    let context = glib::MainContext::new();
    context
      .with_thread_default(|| {
        let (sender, receiver) = async_channel::unbounded();
        for value in 0..10 {
          sender.try_send(value).unwrap();
        }
        let received = Rc::new(RefCell::new(Vec::new()));
        let output = received.clone();
        let task = attach(&context, receiver, move |value| {
          output.borrow_mut().push(value);
          if value == 1 {
            glib::ControlFlow::Break
          } else {
            glib::ControlFlow::Continue
          }
        });
        context.block_on(task).unwrap();
        assert_eq!(*received.borrow(), vec![0, 1]);
        assert!(matches!(
          sender.try_send(10),
          Err(async_channel::TrySendError::Closed(10))
        ));
      })
      .unwrap();
  }

  #[test]
  fn busy_channel_yields_to_other_main_context_work() {
    let context = glib::MainContext::new();
    context
      .with_thread_default(|| {
        let (sender, receiver) = async_channel::unbounded();
        for value in 0..4096 {
          sender.try_send(value).unwrap();
        }
        drop(sender);
        let count = Rc::new(Cell::new(0));
        let observed = Rc::new(Cell::new(usize::MAX));
        let context_for_callback = context.clone();
        let seen = count.clone();
        let sample = observed.clone();
        let task = attach(&context, receiver, move |_| {
          seen.set(seen.get() + 1);
          if seen.get() == 1 {
            let seen = seen.clone();
            let sample = sample.clone();
            context_for_callback.spawn_local(async move {
              sample.set(seen.get());
            });
          }
          glib::ControlFlow::Continue
        });
        context.block_on(task).unwrap();
        assert_eq!(count.get(), 4096);
        assert!(
          observed.get() > 0 && observed.get() <= 64,
          "observed {}",
          observed.get()
        );
      })
      .unwrap();
  }
  #[test]
  #[ignore = "requires a GTK display; run under xvfb-run with --include-ignored"]
  fn real_window_requests_from_a_worker_reach_gtk_in_order() {
    use crate::{
      event_loop::EventLoopBuilder,
      platform::unix::{EventLoopBuilderExtUnix, WindowExtUnix},
      window::WindowBuilder,
    };
    use gtk::prelude::*;
    let event_loop = EventLoopBuilder::<()>::new().with_any_thread(true).build();
    let window = std::sync::Arc::new(
      WindowBuilder::new()
        .with_visible(false)
        .build(&event_loop)
        .unwrap(),
    );
    let worker_window = window.clone();
    std::thread::spawn(move || {
      for value in 0..128 {
        worker_window.set_title(&format!("channel-{value}"));
      }
      worker_window.set_resizable(false);
      worker_window.set_visible(true);
    })
    .join()
    .unwrap();
    let context = glib::MainContext::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let gtk_window = window.gtk_window();
    loop {
      context.iteration(false);
      if gtk_window.title().as_deref() == Some("channel-127")
        && !gtk_window.is_resizable()
        && gtk_window.is_visible()
      {
        break;
      }
      assert!(
        std::time::Instant::now() < deadline,
        "window requests failed to reach GTK"
      );
      std::thread::sleep(std::time::Duration::from_millis(1));
    }
  }
}

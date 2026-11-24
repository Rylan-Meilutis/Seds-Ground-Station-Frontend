//! Recovery search on the watch, relayed by the phone
//! (docs/watch-companion.md "Recovery search").
//!
//! The watch is its own searcher device (`<phone device id>-w`, "<user>'s
//! watch"). Its user opts in on the watch (`search_share`); the phone sends
//! that opt-in to the station and reports the result in the snapshot's
//! `search` object. While the search runs and the watch is opted in, the
//! watch records fixes and sends them in `search_track` messages with its
//! own sequence numbers. The phone takes custody of them in a persisted
//! outbox (the same one the phone uses for itself) and acknowledges the
//! highest contiguous sequence number in the snapshot, after which the watch
//! drops them. The station de-duplicates by device and sequence number, so a
//! resend after a lost acknowledgement is harmless.
//!
//! The phone never starts the watch recording: only the watch user's own
//! toggle does, and the phone refuses fixes for any other session or after
//! the opt-in ended.
use super::super::recovery_search::{self as model, Fix, OptInReply, Outbox, Status, UploadReply};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Mutex;

pub(crate) const STORE_KEY: &str = "recovery_search_watch_outbox_v1";
/// Most fixes one `search_track` message may carry.
pub(crate) const MAX_MESSAGE_POINTS: usize = 200;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct WatchFix {
    pub seq: u64,
    pub t_ms: i64,
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub acc_m: Option<f64>,
}

#[derive(Default)]
pub(crate) struct Relay {
    pub outbox: Option<Outbox>,
    /// Pending opt-in changes from the watch (the last one wins).
    pub share_request: Option<bool>,
    /// Highest contiguous watch sequence number in the phone's custody.
    pub acked_seq: u64,
    pub dirty: bool,
}

impl Relay {
    /// Takes custody of fixes for the watch's current opt-in. Returns the
    /// new acknowledged sequence number.
    pub(crate) fn accept(&mut self, session_id: &str, points: Vec<WatchFix>) -> u64 {
        let Some(outbox) = self.outbox.as_mut() else { return self.acked_seq };
        if !outbox.armed() || outbox.session_id.as_deref() != Some(session_id) {
            return self.acked_seq;
        }
        let mut points = points;
        points.sort_by_key(|p| p.seq);
        for p in points.into_iter().take(MAX_MESSAGE_POINTS) {
            if p.seq <= self.acked_seq || !model::usable(p.lat, p.lon, p.acc_m) {
                // Already held (or worthless): acknowledged so the watch drops it.
                if p.seq == self.acked_seq + 1 {
                    self.acked_seq = p.seq;
                }
                continue;
            }
            if outbox.pending.iter().any(|f| f.seq == p.seq) {
                continue;
            }
            outbox.pending.push_back(Fix { seq: p.seq, t_ms: p.t_ms, lat: p.lat, lon: p.lon, acc_m: p.acc_m });
            if p.seq == self.acked_seq + 1 {
                self.acked_seq = p.seq;
            }
            self.dirty = true;
        }
        outbox.pending.make_contiguous().sort_by_key(|f| f.seq);
        while outbox.pending.len() > model::MAX_PENDING {
            outbox.pending.pop_front();
            outbox.dropped += 1;
        }
        // Later fixes that are now contiguous.
        while outbox.pending.iter().any(|f| f.seq == self.acked_seq + 1) {
            self.acked_seq += 1;
        }
        self.acked_seq
    }

    /// The snapshot's `search` object for the watch.
    pub(crate) fn snapshot(&self, status: Option<&Status>) -> Value {
        let watch = self.outbox.as_ref();
        json!({
            "active": status.is_some_and(|s| s.active),
            "session_id": status.and_then(|s| s.session_id.clone()),
            "sharing": watch.is_some_and(|o| o.armed() && status.is_some_and(|s| s.active && s.session_id == o.session_id)),
            "acked_seq": self.acked_seq,
            "percent": status.and_then(|s| s.stats.percent_covered),
            "swath_m": status.map(|s| s.settings.swath_width_m),
        })
    }
}

pub(crate) static RELAY: Mutex<Relay> = Mutex::new(Relay { outbox: None, share_request: None, acked_seq: 0, dirty: false });
static STATUS_FOR_WATCH: Mutex<Option<Status>> = Mutex::new(None);

/// A watch message: the user toggled sharing on the watch.
pub(crate) fn on_share(sharing: bool) {
    if let Ok(mut relay) = RELAY.lock() {
        relay.share_request = Some(sharing);
    }
}

/// A watch message: buffered fixes.
pub(crate) fn on_track(session_id: String, points: Vec<WatchFix>) {
    if let Ok(mut relay) = RELAY.lock() {
        relay.accept(&session_id, points);
    }
}

/// Adds the `search` object to an outgoing snapshot (only once the phone
/// has seen a search status, so watches without the feature see no change).
pub(crate) fn annotate(snapshot: &mut Value) {
    let status = STATUS_FOR_WATCH.lock().ok().and_then(|s| s.clone());
    if status.is_none() {
        return;
    }
    if let (Ok(relay), Some(map)) = (RELAY.lock(), snapshot.as_object_mut()) {
        map.insert("search".into(), relay.snapshot(status.as_ref()));
    }
}

pub(crate) fn pending() -> usize {
    RELAY.lock().ok().and_then(|r| r.outbox.as_ref().map(|o| o.pending.len())).unwrap_or(0)
}

fn owner_key() -> Option<(String, String)> {
    let session = crate::auth::current_session()?;
    if session.session.anonymous {
        return None;
    }
    let user = session.session.username.clone().filter(|u| !u.is_empty())?;
    Some((format!("{}|{}|watch", super::super::UrlConfig::base_http(), user), user))
}

fn watch_device_id() -> String {
    let phone = super::super::persist::get_string(model::DEVICE_ID_KEY).unwrap_or_else(|| "srch-phone".into());
    format!("{}-w", phone.chars().take(60).collect::<String>())
}

/// One relay step, run by the phone's recovery search runtime every second:
/// follows the station status, sends the watch's opt-in changes, uploads the
/// watch's buffered fixes.
pub(crate) async fn relay_once() {
    use dioxus::prelude::ReadableExt;
    let status = super::super::recovery_search_view::STATUS.peek().clone();
    if let Ok(mut slot) = STATUS_FOR_WATCH.lock() {
        *slot = status.clone();
    }
    let Some((key, user)) = owner_key() else { return };
    let (share, batch) = {
        let Ok(mut relay) = RELAY.lock() else { return };
        if relay.outbox.as_ref().is_none_or(|o| o.owner != key) {
            let stored = super::super::persist::get_string(STORE_KEY).and_then(|raw| Outbox::parse(&raw)).filter(|o| o.owner == key);
            relay.acked_seq = stored.as_ref().map_or(0, |o| o.next_seq.saturating_sub(1));
            relay.outbox = Some(stored.unwrap_or_else(|| Outbox::new(&key, &watch_device_id())));
        }
        let outbox = relay.outbox.as_mut().unwrap();
        if let Some(status) = status.as_ref() {
            // The station decides; the watch's opt-in ends with the session.
            if !status.accepting_uploads || status.session_id != outbox.session_id {
                outbox.disarm();
            }
        }
        let batch = outbox.batch(super::super::current_wallclock_ms());
        (relay.share_request.take(), batch)
    };
    if let Some(sharing) = share {
        let device_id = RELAY.lock().ok().and_then(|r| r.outbox.as_ref().map(|o| o.device_id.clone())).unwrap_or_default();
        let request = model::OptInRequest {
            device_id,
            device_name: super::super::proximity_lock::default_device_name(&user, super::super::proximity_lock::Platform::Watch),
            platform: "watch".into(),
            sharing,
            delete_my_track: false,
        };
        if !sharing && let Ok(mut relay) = RELAY.lock() {
            // Stop at once on the phone too.
            if let Some(o) = relay.outbox.as_mut() {
                o.disarm();
            }
            relay.dirty = true;
        }
        match super::super::http_post_json::<_, OptInReply>(model::OPT_IN_PATH, &request).await {
            Ok(reply) => {
                if let Ok(mut relay) = RELAY.lock() {
                    if let (true, Some(session), Some(id), Some(o)) = (reply.sharing, reply.session_id.as_deref(), reply.opt_in_id.as_deref(), relay.outbox.as_mut()) {
                        o.arm(session, id, reply.max_seq);
                        // The watch numbers its own fixes; resume after what the station has.
                        let held = reply.max_seq.unwrap_or(0);
                        relay.acked_seq = relay.acked_seq.max(held);
                    }
                    relay.dirty = true;
                }
                super::PUBLISH_REQUESTED.store(true, std::sync::atomic::Ordering::Release);
            }
            Err(_) if sharing => {
                // Not opted in: the watch keeps showing "not sharing".
                super::PUBLISH_REQUESTED.store(true, std::sync::atomic::Ordering::Release);
            }
            Err(_) => {}
        }
    }
    if let Some(batch) = batch {
        let reply = super::super::http_post_json::<_, UploadReply>(model::TRACK_PATH, &batch).await;
        if let Ok(mut relay) = RELAY.lock()
            && let Some(o) = relay.outbox.as_mut()
        {
            match &reply {
                Ok(_) => o.sent(&batch),
                Err(error) => {
                    o.failed(&batch, error, super::super::current_wallclock_ms());
                }
            }
            relay.dirty = true;
        }
    }
    let raw = RELAY.lock().ok().and_then(|mut relay| {
        if !relay.dirty {
            return None;
        }
        relay.dirty = false;
        let acked = relay.acked_seq;
        relay.outbox.as_mut().map(|o| {
            // Persist the acknowledged position with the buffer.
            o.next_seq = o.next_seq.max(acked + 1);
            serde_json::to_string(o).unwrap_or_default()
        })
    });
    if let Some(raw) = raw {
        super::super::persist::set_string(STORE_KEY, &raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(seq: u64) -> WatchFix {
        WatchFix { seq, t_ms: seq as i64 * 1000, lat: 31.0 + seq as f64 * 1e-4, lon: -102.0, acc_m: Some(5.0) }
    }

    fn armed() -> Relay {
        let mut outbox = Outbox::new("gs|ada|watch", "srch-x-w");
        outbox.arm("rs-1", "opt", None);
        Relay { outbox: Some(outbox), ..Relay::default() }
    }

    #[test]
    fn the_phone_relays_only_for_the_watchs_opt_in_and_session() {
        let mut relay = Relay { outbox: Some(Outbox::new("gs|ada|watch", "w")), ..Relay::default() };
        assert_eq!(relay.accept("rs-1", vec![fix(1)]), 0, "the watch is not opted in");
        let mut relay = armed();
        assert_eq!(relay.accept("rs-2", vec![fix(1)]), 0, "another session");
        assert!(relay.outbox.as_ref().unwrap().pending.is_empty());
    }

    #[test]
    fn out_of_order_and_repeated_watch_batches_are_held_once_and_acknowledged_contiguously() {
        let mut relay = armed();
        assert_eq!(relay.accept("rs-1", vec![fix(1), fix(2)]), 2);
        // 4 and 5 arrive before 3 (a lost message): held, not yet acknowledged.
        assert_eq!(relay.accept("rs-1", vec![fix(5), fix(4)]), 2);
        assert_eq!(relay.accept("rs-1", vec![fix(3)]), 5);
        // The watch resends after missing the acknowledgement.
        assert_eq!(relay.accept("rs-1", vec![fix(1), fix(2), fix(3), fix(4), fix(5)]), 5);
        let seqs: Vec<u64> = relay.outbox.as_ref().unwrap().pending.iter().map(|f| f.seq).collect();
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
        // The upload batch is the watch's own sequence numbers.
        let batch = relay.outbox.as_ref().unwrap().batch(0).unwrap();
        assert_eq!(batch.device_id, "srch-x-w");
        assert_eq!(batch.points.len(), 5);
    }

    #[test]
    fn the_snapshot_tells_the_watch_whether_it_may_record() {
        let mut relay = armed();
        relay.accept("rs-1", vec![fix(1)]);
        let status: Status = serde_json::from_value(json!({"active": true, "session_id": "rs-1", "accepting_uploads": true, "stats": {"percent_covered": 12.5}})).unwrap();
        let s = relay.snapshot(Some(&status));
        assert_eq!(s["sharing"], true);
        assert_eq!(s["acked_seq"], 1);
        assert_eq!(s["percent"], 12.5);
        let stopped: Status = serde_json::from_value(json!({"active": false, "session_id": "rs-1", "accepting_uploads": true})).unwrap();
        assert_eq!(relay.snapshot(Some(&stopped))["sharing"], false, "no recording after the stop");
        assert_eq!(Relay::default().snapshot(None)["sharing"], false);
    }
}

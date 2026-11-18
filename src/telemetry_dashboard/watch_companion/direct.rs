//! Direct watch link provisioning. iOS suspends the phone app in the
//! background, so the watch also reads the station itself. The phone gives
//! it what it needs over the watch transport's queued channels
//! (WatchConnectivity user info + application context; a Wear Data Layer
//! item), which arrive even while the phone app is suspended later:
//!
//! * the station address the phone uses;
//! * a scoped, revocable watch token issued by the GS for the signed-in
//!   account (`POST /api/auth/watch-token`). It reads `/api/watch/status`
//!   and sends only the actions configured on watch pages; the password is
//!   never copied.
//!
//! Signing out on the phone revokes the token on the GS (logout cascades)
//! and the watch is told to forget it; turning off "Direct link" revokes it
//! explicitly.
use serde::{Deserialize, Serialize};

pub const TOKEN_ENDPOINT: &str = "/api/auth/watch-token";
pub const REVOKE_ENDPOINT: &str = "/api/auth/watch-token/revoke";
/// Random id of this phone install, so a new token replaces this phone's
/// previous one on the GS without touching other phones' watches.
pub const DEVICE_ID_KEY: &str = "watch_direct_device_id_v1";
/// Reissue this long before the token expires.
pub const REFRESH_BEFORE_EXPIRY_MS: i64 = 3 * 24 * 3600 * 1000;
/// Wait between failed issue attempts.
pub const RETRY_MS: i64 = 60_000;

/// `POST /api/auth/watch-token` reply.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Grant {
    pub token: String,
    pub device_id: String,
    pub expires_at_ms: i64,
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Serialize)]
pub struct IssueRequest<'a> {
    pub device_id: &'a str,
    pub commands: &'a [String],
}

#[derive(Serialize)]
pub struct RevokeRequest<'a> {
    pub device_id: &'a str,
}

/// What a token is wanted for. A change of station, account or the set of
/// page actions needs a new token.
#[derive(Clone, Debug, PartialEq)]
pub struct Want {
    /// `base|username`.
    pub account: String,
    pub base: String,
    pub username: String,
    pub station: String,
    pub skip_tls: bool,
    pub commands: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub want: Want,
    pub grant: Grant,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Ask the GS for a (new) token.
    Issue,
    /// Nothing to do.
    Keep,
    /// No longer wanted: revoke on the GS when the session still allows it
    /// and tell the watch to forget the token.
    Drop,
}

/// Decides what to do about the watch token.
pub fn plan(held: Option<&Held>, want: Option<&Want>, now_ms: i64, last_failure_ms: Option<i64>) -> Step {
    let retry_ok = last_failure_ms.is_none_or(|t| now_ms.saturating_sub(t) >= RETRY_MS);
    match (held, want) {
        (None, None) => Step::Keep,
        (Some(_), None) => Step::Drop,
        (None, Some(_)) => {
            if retry_ok {
                Step::Issue
            } else {
                Step::Keep
            }
        }
        (Some(held), Some(want)) => {
            let stale = held.want != *want
                || held.grant.expires_at_ms.saturating_sub(now_ms) < REFRESH_BEFORE_EXPIRY_MS;
            if stale && retry_ok { Step::Issue } else { Step::Keep }
        }
    }
}

/// The phone → watch provisioning message (`type: direct`). Without a token
/// the watch deletes any it holds and stops polling the station.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Provision {
    pub v: u32,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub enabled: bool,
    /// Base URLs the watch may use, best first.
    pub addresses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
    pub username: String,
    pub station: String,
    /// The phone accepts this station's certificate without verification
    /// (its "skip TLS verification" setting); the watch does the same for
    /// these addresses only.
    pub skip_tls: bool,
    /// Commands the token may send.
    pub commands: Vec<String>,
}

pub fn provision(held: Option<&Held>) -> Provision {
    match held {
        Some(held) => Provision {
            v: super::protocol::PROTOCOL_VERSION,
            kind: "direct",
            enabled: true,
            addresses: vec![held.want.base.trim_end_matches('/').to_string()],
            token: Some(held.grant.token.clone()),
            expires_at_ms: Some(held.grant.expires_at_ms),
            username: held.want.username.clone(),
            station: held.want.station.clone(),
            skip_tls: held.want.skip_tls,
            commands: held.grant.commands.clone(),
        },
        None => Provision {
            v: super::protocol::PROTOCOL_VERSION,
            kind: "direct",
            enabled: false,
            addresses: Vec::new(),
            token: None,
            expires_at_ms: None,
            username: String::new(),
            station: String::new(),
            skip_tls: false,
            commands: Vec::new(),
        },
    }
}

/// A random id for this phone install (stable once stored).
pub fn new_device_id(random: u128) -> String {
    format!("phone-{random:032x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn want(commands: &[&str]) -> Want {
        Want {
            account: "https://gs.local:3000|ada".into(),
            base: "https://gs.local:3000/".into(),
            username: "ada".into(),
            station: "GroundStation".into(),
            skip_tls: false,
            commands: commands.iter().map(|c| c.to_string()).collect(),
        }
    }

    fn held(w: Want, expires_at_ms: i64) -> Held {
        Held {
            grant: Grant {
                token: "gsw1.abc".into(),
                device_id: "phone-1".into(),
                expires_at_ms,
                commands: w.commands.clone(),
            },
            want: w,
        }
    }

    const DAY: i64 = 24 * 3600 * 1000;

    #[test]
    fn issue_refresh_and_drop() {
        let now = 100 * DAY;
        assert_eq!(plan(None, None, now, None), Step::Keep);
        assert_eq!(plan(None, Some(&want(&[])), now, None), Step::Issue);
        // Failed recently: wait before retrying.
        assert_eq!(plan(None, Some(&want(&[])), now, Some(now - 1_000)), Step::Keep);
        assert_eq!(plan(None, Some(&want(&[])), now, Some(now - RETRY_MS)), Step::Issue);
        let h = held(want(&["Abort"]), now + 10 * DAY);
        assert_eq!(plan(Some(&h), Some(&want(&["Abort"])), now, None), Step::Keep);
        // Page actions changed: the token must carry the new set.
        assert_eq!(plan(Some(&h), Some(&want(&["Abort", "Dump"])), now, None), Step::Issue);
        // Another station or account.
        let mut other = want(&["Abort"]);
        other.account = "https://rrls.local:3000|ada".into();
        assert_eq!(plan(Some(&h), Some(&other), now, None), Step::Issue);
        // Close to expiry.
        let expiring = held(want(&["Abort"]), now + 2 * DAY);
        assert_eq!(plan(Some(&expiring), Some(&want(&["Abort"])), now, None), Step::Issue);
        // Signed out, not eligible or direct link off.
        assert_eq!(plan(Some(&h), None, now, None), Step::Drop);
    }

    #[test]
    fn provisioning_carries_address_and_token_but_never_a_password() {
        let h = held(want(&["Abort"]), 5);
        let p = provision(Some(&h));
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["type"], "direct");
        assert_eq!(json["enabled"], true);
        assert_eq!(json["addresses"], serde_json::json!(["https://gs.local:3000"]));
        assert_eq!(json["token"], "gsw1.abc");
        assert_eq!(json["commands"], serde_json::json!(["Abort"]));
        let text = json.to_string();
        assert!(!text.contains("password") && !text.contains("proof"));
        let off = serde_json::to_value(provision(None)).unwrap();
        assert_eq!(off["enabled"], false);
        assert!(off.get("token").is_none());
        assert_eq!(new_device_id(255), "phone-000000000000000000000000000000ff");
    }
}

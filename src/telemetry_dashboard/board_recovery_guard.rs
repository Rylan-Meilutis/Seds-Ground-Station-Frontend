//! Local confirmation binding complements the server's authoritative ticket checks.
//! Tokens stay in memory and are never serialized into UI, logs or request bodies.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct ConnectionBinding {
    pub backend: String,
    pub token: Option<String>,
    pub principal: Option<String>,
}
impl ConnectionBinding {
    pub fn current() -> Self {
        Self {
            backend: super::UrlConfig::base_http(),
            token: crate::auth::current_token(),
            principal: crate::auth::current_status().username,
        }
    }
}

/// Count the whole local request/transport interval conservatively. A clock
/// discontinuity is never allowed to make a board sample look younger.
pub(super) fn status_age_ms(request_started_ms: f64, now_ms: f64) -> u64 {
    if !request_started_ms.is_finite() || !now_ms.is_finite() || now_ms < request_started_ms {
        return u64::MAX;
    }
    (now_ms - request_started_ms).ceil() as u64
}

pub(super) fn challenge_unexpired(received_ms: f64, lifetime_ms: u64, now_ms: f64) -> bool {
    received_ms.is_finite()
        && now_ms.is_finite()
        && now_ms >= received_ms
        && now_ms - received_ms < lifetime_ms as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_cannot_follow_backend_auth_or_principal_changes() {
        let captured = ConnectionBinding {
            backend: "https://station-one.invalid".into(),
            token: Some("fixture-session".into()),
            principal: Some("fixture-operator".into()),
        };
        assert!(captured == captured.clone());
        let mut next = captured.clone();
        next.backend = "https://station-two.invalid".into();
        assert!(captured != next);
        next = captured.clone();
        next.token = None;
        assert!(captured != next);
        next = captured.clone();
        next.token = Some("replacement-session".into());
        assert!(captured != next);
        next = captured.clone();
        next.principal = Some("another-operator".into());
        assert!(captured != next);
    }
    #[test]
    fn request_age_includes_transport_and_fails_closed_on_discontinuity() {
        assert_eq!(status_age_ms(100.0, 100.0), 0);
        assert_eq!(status_age_ms(100.0, 2600.0), 2500);
        assert_eq!(status_age_ms(100.0, 100.1), 1);
        assert_eq!(status_age_ms(100.0, 99.0), u64::MAX);
        assert_eq!(status_age_ms(f64::NAN, 100.0), u64::MAX);
        assert_eq!(status_age_ms(100.0, f64::INFINITY), u64::MAX);
    }
    #[test]
    fn challenge_expiry_is_non_sliding_and_fails_closed_on_clock_regression() {
        assert!(challenge_unexpired(100.0, 5_000, 100.0));
        assert!(challenge_unexpired(100.0, 5_000, 5_099.0));
        assert!(!challenge_unexpired(100.0, 5_000, 5_100.0));
        assert!(!challenge_unexpired(100.0, 5_000, 99.0));
        assert!(!challenge_unexpired(100.0, 0, 100.0));
        assert!(!challenge_unexpired(f64::NAN, 5_000, 100.0));
        assert!(!challenge_unexpired(100.0, 5_000, f64::INFINITY));
    }
}

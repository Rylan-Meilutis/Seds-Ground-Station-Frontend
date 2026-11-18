//! What the phone app is doing about a station, sent to the watch while no
//! dashboard is mounted (Connect screen, "Checking session…", sign-in,
//! connection failure), so the watch can show "Connecting to <address>"
//! instead of nothing.
use super::bridge::LinkState;
use super::protocol::Station;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum PhonePhase {
    /// Connect screen: no station chosen or the user is picking one.
    #[default]
    ChoosingStation,
    /// Contacting the station / checking the session at `base`.
    Connecting { base: String },
    /// The station answered and needs a login on the phone.
    SignInRequired { base: String },
    /// The station could not be reached.
    Unreachable { base: String },
}

static PHONE_PHASE: Mutex<PhonePhase> = Mutex::new(PhonePhase::ChoosingStation);

/// Records the phone's connection phase; a change is sent to the watch at
/// once.
pub(crate) fn set_phone_phase(phase: PhonePhase) {
    let Ok(mut slot) = PHONE_PHASE.lock() else {
        return;
    };
    if *slot != phase {
        *slot = phase;
        super::PUBLISH_REQUESTED.store(true, Ordering::Release);
    }
}

pub(crate) fn phone_phase() -> PhonePhase {
    PHONE_PHASE.lock().map(|g| g.clone()).unwrap_or_default()
}

/// `host[:port]` of a base URL such as `https://gs.local:3000/`.
pub(crate) fn station_address(base: &str) -> String {
    let rest = base.trim();
    let rest = rest.split_once("://").map_or(rest, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // Drop any user:password@ prefix.
    host.rsplit_once('@').map_or(host, |(_, h)| h).to_owned()
}

/// The `station` block of a snapshot sent without a dashboard.
pub(crate) fn station_for_phase(phase: &PhonePhase) -> Station {
    let (link, base) = match phase {
        PhonePhase::ChoosingStation => ("offline", ""),
        PhonePhase::Connecting { base } => ("connecting", base.as_str()),
        PhonePhase::SignInRequired { base } => ("sign_in", base.as_str()),
        PhonePhase::Unreachable { base } => ("offline", base.as_str()),
    };
    Station {
        name: String::new(),
        role: "unknown".into(),
        link,
        address: station_address(base),
        telemetry_age_ms: None,
    }
}

/// Why the Watch settings tab is or is not shown, for the diagnostics line.
pub(crate) fn eligibility_reason(
    companion_platform: bool,
    signed_in: bool,
    link: LinkState,
) -> &'static str {
    if !companion_platform || !link.supported {
        "hidden: this build has no watch bridge (iOS and Android apps only)"
    } else if !signed_in {
        "hidden: sign in with a named account"
    } else if !link.paired && !link.activated {
        "hidden: watch service still activating"
    } else if !link.paired {
        "hidden: no paired watch"
    } else if !link.installed {
        "hidden: GS watch app not installed on the paired watch"
    } else {
        "shown"
    }
}

#[cfg(test)]
mod tests {
    use super::super::settings_eligible;
    use super::*;

    #[test]
    fn addresses_are_host_and_port() {
        assert_eq!(station_address("https://gs.local:3000/"), "gs.local:3000");
        assert_eq!(
            station_address("http://10.0.0.5:3000/api?x=1"),
            "10.0.0.5:3000"
        );
        assert_eq!(
            station_address("https://user:pw@example.org"),
            "example.org"
        );
        assert_eq!(station_address("gs.local"), "gs.local");
        assert_eq!(station_address(""), "");
    }

    #[test]
    fn every_phase_tells_the_watch_where_the_phone_is() {
        let base = "https://gs.local:3000".to_owned();
        let cases = [
            (PhonePhase::ChoosingStation, "offline", ""),
            (
                PhonePhase::Connecting { base: base.clone() },
                "connecting",
                "gs.local:3000",
            ),
            (
                PhonePhase::SignInRequired { base: base.clone() },
                "sign_in",
                "gs.local:3000",
            ),
            (PhonePhase::Unreachable { base }, "offline", "gs.local:3000"),
        ];
        for (phase, link, address) in cases {
            let station = station_for_phase(&phase);
            assert_eq!(
                (station.link, station.address.as_str()),
                (link, address),
                "{phase:?}"
            );
        }
    }

    #[test]
    fn phase_changes_request_a_publish() {
        super::super::PUBLISH_REQUESTED.store(false, Ordering::Release);
        set_phone_phase(PhonePhase::Connecting {
            base: "https://phase-test".into(),
        });
        assert!(super::super::PUBLISH_REQUESTED.swap(false, Ordering::AcqRel));
        assert_eq!(
            phone_phase(),
            PhonePhase::Connecting {
                base: "https://phase-test".into()
            }
        );
        set_phone_phase(PhonePhase::Connecting {
            base: "https://phase-test".into(),
        });
        assert!(!super::super::PUBLISH_REQUESTED.load(Ordering::Acquire));
        set_phone_phase(PhonePhase::ChoosingStation);
    }

    /// The iOS bridge reports only the bridge bit until WCSession activation
    /// completes; the tab must appear as soon as the activated state arrives,
    /// and reachability never matters.
    #[test]
    fn activation_order_and_reasons() {
        let before = LinkState::from_bits(1);
        assert!(!settings_eligible(true, true, before));
        assert_eq!(
            eligibility_reason(true, true, before),
            "hidden: watch service still activating"
        );
        let installed_not_reachable = LinkState::from_bits(1 | 2 | 4 | 16);
        assert!(settings_eligible(true, true, installed_not_reachable));
        assert_eq!(
            eligibility_reason(true, true, installed_not_reachable),
            "shown"
        );
        assert_eq!(
            eligibility_reason(true, false, installed_not_reachable),
            "hidden: sign in with a named account"
        );
        assert_eq!(
            eligibility_reason(true, true, LinkState::from_bits(1 | 16)),
            "hidden: no paired watch"
        );
        assert_eq!(
            eligibility_reason(true, true, LinkState::from_bits(1 | 2 | 16)),
            "hidden: GS watch app not installed on the paired watch"
        );
        assert!(
            eligibility_reason(false, true, LinkState::from_bits(31))
                .starts_with("hidden: this build")
        );
        // The reason and the gate never disagree.
        for bits in 0..32 {
            for signed_in in [false, true] {
                let link = LinkState::from_bits(bits);
                assert_eq!(
                    eligibility_reason(true, signed_in, link) == "shown",
                    settings_eligible(true, signed_in, link),
                    "bits={bits} signed_in={signed_in}"
                );
            }
        }
    }
}

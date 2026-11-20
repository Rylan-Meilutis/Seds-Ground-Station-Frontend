//! Opt-in watch page actions. The phone never sends a command for the watch:
//! it only decides which configured actions the watch may show (those the GS
//! lists for this account in `GET /api/watch/actions`) and reports their
//! enable state from the live action policy. The watch sends a press itself,
//! to the GS's ordinary `POST /api/command`, with its scoped watch token.
use super::protocol::{ActionState, Page};
use super::settings::{MAX_PAGE_ACTIONS, WatchSettings};
use crate::telemetry_dashboard::ActionPolicyMsg;
use serde::Deserialize;
use std::collections::BTreeMap;

pub const CATALOG_ENDPOINT: &str = "/api/watch/actions";
/// Most commands one watch token carries (GS `auth::MAX_WATCH_COMMANDS`).
pub const MAX_TOKEN_COMMANDS: usize = 16;

/// Launch- and pyro-class commands. The GS already leaves them out of the
/// watch catalog; the phone refuses them as well unless the GS explicitly
/// marks the entry `watch_safe`.
pub const LAUNCH_AND_PYRO: &[&str] = &[
    "Launch",
    "GroundStationLaunch",
    "ResetLaunchLatch",
    "TogglePhysicalLaunchMode",
    "ToggleLaunchInterlock",
    "Igniter",
    "IgniterSequence",
    "DeployParachute",
    "ExpandParachute",
];

/// One entry of `GET /api/watch/actions`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct CatalogAction {
    pub cmd: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub abort: bool,
    #[serde(default)]
    pub watch_safe: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ExcludedAction {
    pub cmd: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ActionCatalog {
    #[serde(default)]
    pub actions: Vec<CatalogAction>,
    #[serde(default)]
    pub excluded: Vec<ExcludedAction>,
}

impl ActionCatalog {
    /// Entries the watch may carry: the GS list minus Launch/pyro without an
    /// explicit watch-safe mark. Labels fall back to the command name.
    pub fn sanitized(mut self) -> Self {
        self.actions
            .retain(|a| super::settings::command_name_valid(&a.cmd) && allowed_class(a));
        for a in &mut self.actions {
            if a.label.trim().is_empty() {
                a.label = a.cmd.clone();
            }
        }
        self
    }

    #[cfg(test)]
    pub fn find(&self, cmd: &str) -> Option<&CatalogAction> {
        self.actions.iter().find(|a| a.cmd == cmd)
    }
}

fn allowed_class(action: &CatalogAction) -> bool {
    action.watch_safe || !LAUNCH_AND_PYRO.contains(&action.cmd.as_str())
}

/// The page's configured actions that `catalog` offers, in the page's
/// order, at most two. An action the account may not run, that this station
/// mode does not have, or that is Launch/pyro class is dropped.
pub fn permitted<'a>(page_actions: &[String], catalog: &'a [CatalogAction]) -> Vec<&'a CatalogAction> {
    let mut out: Vec<&CatalogAction> = Vec::new();
    for cmd in page_actions {
        if out.len() == MAX_PAGE_ACTIONS {
            break;
        }
        if let Some(entry) = catalog.iter().find(|a| &a.cmd == cmd)
            && allowed_class(entry)
            && !out.iter().any(|a| a.cmd == entry.cmd)
        {
            out.push(entry);
        }
    }
    out
}

/// Commands the watch token must carry: every permitted action on an
/// enabled page, deduplicated in page order.
pub fn token_commands(settings: &WatchSettings, catalog: &[CatalogAction]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for page in settings.pages.iter().filter(|p| p.enabled) {
        for action in permitted(&page.actions, catalog) {
            if !out.contains(&action.cmd) && out.len() < MAX_TOKEN_COMMANDS {
                out.push(action.cmd.clone());
            }
        }
    }
    out
}

/// The GS action policy's state for one command, as the watch shows it.
/// Abort is always pressable: the GS admits a manual Abort under every
/// policy.
pub fn state_from_policy(cmd: &str, policy: &ActionPolicyMsg) -> ActionState {
    if cmd == "Abort" {
        return ActionState {
            enabled: true,
            reason: None,
        };
    }
    let control = policy.controls.iter().find(|c| c.cmd == cmd);
    let enabled = policy.software_buttons_enabled && control.is_some_and(|c| c.enabled);
    let reason = (!enabled).then(|| {
        control
            .and_then(|c| c.disabled_reason.clone())
            .unwrap_or_else(|| {
                if control.is_none() {
                    "Not available in this station mode.".to_string()
                } else {
                    "Software controls are disabled by GroundStation.".to_string()
                }
            })
    });
    ActionState { enabled, reason }
}

/// Enable state of every action on `pages`.
pub fn states(pages: &[Page], policy: Option<&ActionPolicyMsg>) -> BTreeMap<String, ActionState> {
    pages
        .iter()
        .flat_map(|p| p.actions.iter())
        .map(|a| {
            let state = match policy {
                Some(policy) => state_from_policy(&a.cmd, policy),
                None if a.abort => ActionState {
                    enabled: true,
                    reason: None,
                },
                None => ActionState {
                    enabled: false,
                    reason: Some("Waiting for the station's action policy.".into()),
                },
            };
            (a.cmd.clone(), state)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry_dashboard::ActionControl;
    use crate::telemetry_dashboard::watch_companion::protocol::resolved_pages;

    fn entry(cmd: &str) -> CatalogAction {
        CatalogAction {
            cmd: cmd.into(),
            label: format!("{cmd} label"),
            enabled: true,
            ..CatalogAction::default()
        }
    }

    fn control(cmd: &str, enabled: bool, reason: Option<&str>) -> ActionControl {
        serde_json::from_value(serde_json::json!({
            "cmd": cmd, "enabled": enabled, "blink": "none", "actuated": null,
            "disabled_reason": reason,
        }))
        .unwrap()
    }

    #[test]
    fn only_permitted_actions_reach_the_watch() {
        let catalog = vec![entry("Abort"), entry("Dump"), entry("ToggleFlightBuzzer")];
        let actions: Vec<String> = ["Nitrogen", "Dump", "Abort", "ToggleFlightBuzzer"]
            .map(String::from)
            .to_vec();
        // Nitrogen is not offered to this account; at most two survive.
        let ok: Vec<_> = permitted(&actions, &catalog).iter().map(|a| a.cmd.as_str()).collect();
        assert_eq!(ok, vec!["Dump", "Abort"]);
        // No catalog (station without the endpoint, not fetched yet): none.
        assert!(permitted(&actions, &[]).is_empty());
    }

    #[test]
    fn launch_and_pyro_need_an_explicit_watch_safe_mark() {
        let mut launch = entry("Launch");
        let mut igniter = entry("Igniter");
        let catalog = ActionCatalog {
            actions: vec![launch.clone(), igniter.clone(), entry("Abort")],
            excluded: vec![],
        }
        .sanitized();
        assert_eq!(catalog.actions.len(), 1);
        let page = vec!["Launch".to_string(), "Igniter".to_string()];
        assert!(permitted(&page, &[launch.clone(), igniter.clone()]).is_empty());
        launch.watch_safe = true;
        igniter.watch_safe = true;
        assert_eq!(permitted(&page, &[launch, igniter]).len(), 2);
    }

    #[test]
    fn pages_carry_permitted_actions_and_the_token_their_union() {
        let mut s = WatchSettings::default();
        s.toggle_page_action("flight", "Abort");
        s.toggle_page_action("flight", "Dump");
        s.toggle_page_action("propulsion", "Dump");
        s.toggle_page_action("propulsion", "Nitrogen");
        s.toggle_page_action("health", "ToggleFlightBuzzer");
        s.toggle_page("health");
        let catalog = vec![entry("Abort"), entry("Dump"), entry("ToggleFlightBuzzer")];
        let pages = resolved_pages(&s, &catalog);
        let flight = pages.iter().find(|p| p.id == "flight").unwrap();
        assert_eq!(flight.actions.len(), 2);
        assert!(flight.actions[0].abort && !flight.actions[1].abort);
        assert_eq!(flight.actions[1].label, "Dump label");
        let propulsion = pages.iter().find(|p| p.id == "propulsion").unwrap();
        assert_eq!(propulsion.actions.iter().map(|a| a.cmd.as_str()).collect::<Vec<_>>(), vec!["Dump"]);
        // Disabled pages add nothing; duplicates collapse.
        assert_eq!(token_commands(&s, &catalog), vec!["Abort", "Dump"]);
        // Default settings: actions are off.
        assert!(token_commands(&WatchSettings::default(), &catalog).is_empty());
        assert!(resolved_pages(&WatchSettings::default(), &catalog).iter().all(|p| p.actions.is_empty()));
    }

    #[test]
    fn states_follow_the_policy_and_abort_is_always_enabled() {
        let mut s = WatchSettings::default();
        s.toggle_page_action("flight", "Abort");
        s.toggle_page_action("flight", "Dump");
        s.toggle_page_action("trajectory", "ToggleFlightBuzzer");
        let catalog = vec![entry("Abort"), entry("Dump"), entry("ToggleFlightBuzzer")];
        let pages = resolved_pages(&s, &catalog);
        let mut policy = ActionPolicyMsg::default_locked();
        policy.controls = vec![
            control("Abort", false, None),
            control("Dump", false, Some("Fill in progress.")),
            control("ToggleFlightBuzzer", true, None),
        ];
        let states = states(&pages, Some(&policy));
        assert!(states["Abort"].enabled);
        assert_eq!(states["Dump"], ActionState { enabled: false, reason: Some("Fill in progress.".into()) });
        assert!(states["ToggleFlightBuzzer"].enabled);
        policy.software_buttons_enabled = false;
        let states = super::states(&pages, Some(&policy));
        assert!(states["Abort"].enabled);
        assert!(!states["ToggleFlightBuzzer"].enabled);
        let unknown = super::states(&pages, None);
        assert!(unknown["Abort"].enabled && !unknown["Dump"].enabled);
    }

    #[test]
    fn catalog_parses_the_gs_reply() {
        let raw = r#"{"actions":[{"cmd":"Abort","label":"Abort","section":"safety","abort":true,"watch_safe":false,"enabled":true},
            {"cmd":"Dump","label":"","section":"fill_system","abort":false,"watch_safe":false,"enabled":false,"reason":"Fill."}],
            "excluded":[{"cmd":"Launch","reason":"Launch-class actions are not offered on the watch."}],"max_per_page":2}"#;
        let catalog = serde_json::from_str::<ActionCatalog>(raw).unwrap().sanitized();
        assert_eq!(catalog.actions.len(), 2);
        assert_eq!(catalog.find("Dump").unwrap().label, "Dump");
        assert_eq!(catalog.excluded[0].cmd, "Launch");
    }
}

use super::*;
fn ids(tabs: Vec<MainTab>) -> Vec<&'static str> {
    tabs.into_iter().map(_main_tab_to_str).collect()
}
#[test]
fn distinct_configured_views_and_mobile_single_pane() {
    let available = [MainTab::Data, MainTab::Actions, MainTab::Map];
    assert_eq!(
        ids(resolve(MainTab::Data, "actions", &available, true)),
        ["data", "actions"]
    );
    assert_eq!(
        ids(resolve(MainTab::Actions, "actions", &available, true)),
        ["actions", "data"]
    );
    assert_eq!(
        ids(resolve(MainTab::Data, "actions", &available, false)),
        ["data"]
    );
}
#[test]
fn permission_or_configuration_removal_never_mounts_retired_secondary() {
    assert_eq!(
        ids(resolve(
            MainTab::Data,
            "actions",
            &[MainTab::Data, MainTab::Map],
            true
        )),
        ["data", "map"]
    );
    assert_eq!(
        ids(resolve(MainTab::Actions, "data", &[MainTab::Data], true)),
        ["data"]
    );
}
#[test]
fn singleton_editors_and_duplicate_ground_control_surface_stay_out_of_secondary() {
    assert_eq!(
        ids(resolve(
            MainTab::State,
            "actions",
            &[MainTab::State, MainTab::Actions, MainTab::Data],
            true
        )),
        ["state", "data"]
    );
    assert_eq!(
        ids(resolve(
            MainTab::Data,
            "calibration",
            &[MainTab::Data, MainTab::Calibration],
            true
        )),
        ["data"]
    );
}

#[test]
fn reverse_duplicate_prefers_actions_over_an_earlier_configured_connection_view() {
    let available = [
        MainTab::State,
        MainTab::ConnectionStatus,
        MainTab::Actions,
        MainTab::Data,
    ];
    assert_eq!(
        ids(resolve(MainTab::Data, "data", &available, true)),
        ["data", "actions"]
    );
}

#[test]
fn choosing_the_second_view_as_first_swaps_instead_of_dropping_the_first() {
    let available = [MainTab::Actions, MainTab::Map, MainTab::Data];
    let shown = [MainTab::Actions, MainTab::Map];
    let secondary = secondary_choice(&shown, MainTab::Map, "map");
    assert_eq!(secondary, "actions");
    assert_eq!(
        ids(resolve(MainTab::Map, &secondary, &available, true)),
        ["map", "actions"]
    );
    // An unrelated first-pane choice keeps the saved second pane.
    assert_eq!(secondary_choice(&shown, MainTab::Data, "map"), "map");
    assert_eq!(secondary_choice(&[MainTab::Map], MainTab::Map, "map"), "map");
}

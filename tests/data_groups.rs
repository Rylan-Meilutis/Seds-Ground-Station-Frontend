#[path = "../src/telemetry_dashboard/data_groups.rs"]
pub mod data_groups;
#[path = "../src/telemetry_dashboard/layout.rs"]
pub mod layout;

#[cfg(test)]
mod grouped_views {
    use super::{data_groups::with_grouped_views, layout::*};
    use serde_json::{Value, json};

    fn layout(tabs: Value) -> DataTabLayout {
        serde_json::from_value(
            json!({"tabs": tabs, "sender_split_data_types": ["BATTERY_VOLTAGE"],
            "default_display_filter": {"kind": "raw"}}),
        )
        .unwrap()
    }

    fn overview(layout: &DataTabLayout, family: &str) -> DataSubtabSpec {
        layout
            .tabs
            .iter()
            .find(|t| t.id == format!("gs26-overview-{family}"))
            .unwrap()
            .subtabs
            .as_ref()
            .unwrap()[0]
            .clone()
    }

    #[test]
    fn original_custom_tabs_and_global_settings_are_unchanged_and_reentry_is_stable() {
        let original = layout(json!([
            {"id":"GYRO_DATA", "label":"Configured gyro", "channels":["Roll", "Pitch"]},
            {"id":"operator-private", "label":"Custom sensor", "channels":["Unknown units"]}
        ]));
        let result = with_grouped_views(original.clone());
        assert_eq!(
            &result.tabs[..original.tabs.len()],
            original.tabs.as_slice()
        );
        assert_eq!(
            result.sender_split_data_types,
            original.sender_split_data_types
        );
        assert_eq!(
            result.default_display_filter,
            original.default_display_filter
        );
        assert_eq!(with_grouped_views(result.clone()), result);
        assert_eq!(
            result.tabs.len(),
            3,
            "unknown custom sensor creates no guessed family"
        );
    }

    #[test]
    fn explicit_unknown_source_does_not_inherit_a_known_container_family() {
        let input = layout(
            json!([{"id":"LOADCELL", "label":"Sensor container", "channels":[],
            "subtabs":[{"id":"custom", "label":"Custom input", "data_type":"OPERATOR_CUSTOM_SENSOR", "channels":["Unknown units"]}]}]),
        );
        assert_eq!(with_grouped_views(input.clone()), input);
    }

    #[test]
    fn inherited_metadata_is_materialized_without_changing_measurements() {
        let input = layout(
            json!([{"id":"BATTERY_VOLTAGE", "label":"Power", "channels":["V"],
            "show_min_max":false, "channel_formatters":[{"kind":"number","precision":4,"suffix":" V"}],
            "boolean_labels":{"true_label":"On","false_label":"Off","unknown_label":"Unknown"},
            "display_filter":{"kind":"low_pass","cutoff_hz":3.0},
            "chart_groups":[{"title":"Configured voltage axis","channels":[0],"scale_mode":"per_series"}],
            "subtabs":[{"id":"pb", "label":"AV bay", "sender_id":"PB"}]}]),
        );
        let result = with_grouped_views(input);
        let view = overview(&result, "power");
        let item = &view.summary_items.as_ref().unwrap()[0];
        assert_eq!(
            (&item.data_type, item.index, item.sender_id.as_deref()),
            (&"BATTERY_VOLTAGE".to_string(), 0, Some("PB"))
        );
        assert_eq!(
            item.formatter.as_ref().unwrap().suffix.as_deref(),
            Some(" V")
        );
        assert_eq!(item.formatter.as_ref().unwrap().precision, Some(4));
        assert_eq!(
            item.boolean_labels
                .as_ref()
                .unwrap()
                .unknown_label
                .as_deref(),
            Some("Unknown")
        );
        assert_eq!(item.show_min_max, Some(false));
        assert_eq!(item.display_filter.as_ref().unwrap().cutoff_hz, Some(3.0));
        let graph = &view.chart_groups.as_ref().unwrap()[0];
        assert_eq!(graph.scale_mode, Some(DataChartScaleMode::PerSeries));
        assert_eq!(graph.display_filter, item.display_filter);
        assert_eq!(graph.data_type.as_deref(), Some("BATTERY_VOLTAGE"));
        assert_eq!(graph.sender_id.as_deref(), Some("PB"));
        let series = &graph.chart_series.as_ref().unwrap()[0];
        assert_eq!(series.sender_id.as_deref(), Some("PB"));
        assert_eq!(series.label.as_deref(), Some("AV bay · V"));
        assert!(view.channels.as_ref().unwrap().is_empty());
        assert!(
            view.data_type.is_none(),
            "navigation ID must not become a source"
        );
    }

    #[test]
    fn distinct_summary_display_policies_are_preserved() {
        let result = with_grouped_views(layout(json!([
            {"id":"LOADCELL", "label":"Loadcells", "channels":[], "subtabs":[
                {"id":"raw", "label":"Raw", "summary_items":[
                    {"label":"Weight", "data_type":"LOADCELL_50_WEIGHT_KG", "index":0, "show_min_max":true, "display_filter":{"kind":"raw"}},
                    {"label":"Smoothed", "data_type":"LOADCELL_50_WEIGHT_KG", "index":0, "show_min_max":false, "display_filter":{"kind":"low_pass", "cutoff_hz":3.0}}
                ]}
            ]}
        ])));
        let items = overview(&result, "fill").summary_items.unwrap();
        assert_eq!(items.len(), 2);
        assert_ne!(items[0].display_filter, items[1].display_filter);
        assert_ne!(items[0].show_min_max, items[1].show_min_max);
    }

    #[test]
    fn actual_senders_stay_distinct_and_explicit_summary_sources_are_not_rebound() {
        let result = with_grouped_views(layout(
            json!([{"id":"BATTERY_VOLTAGE", "label":"Battery", "channels":["Voltage"],
            "subtabs":[{"id":"pb","label":"AV","sender_id":"PB"},
                {"id":"vb","label":"Valve","sender_id":"VB"},
                {"id":"explicit","label":"Explicit","sender_id":"GB", "chart":{"enabled":false},
                 "summary_items":[{"label":"Unscoped","data_type":"BATTERY_CURRENT","index":2},
                     {"label":"Explicit PB","data_type":"BATTERY_CURRENT","index":2,"sender_id":"PB"}]}]}]),
        ));
        let view = overview(&result, "power");
        let items = view.summary_items.unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].sender_id.as_deref(), Some("PB"));
        assert_eq!(items[1].sender_id.as_deref(), Some("VB"));
        assert_eq!(items[2].sender_id, None);
        assert_eq!(items[3].sender_id.as_deref(), Some("PB"));
        assert_eq!(view.chart_groups.unwrap().len(), 2);
    }

    #[test]
    fn loadcell_daq_copies_dedup_but_different_graph_policies_survive() {
        let sensor = json!({"id":"kg50","label":"50kg","data_type":"KG50","channels":["Raw"],
            "summary_items":[{"label":"Weight","data_type":"LOADCELL_50_WEIGHT_KG","index":0,
                "formatter":{"kind":"number","suffix":" kg"}}],
            "chart_groups":[{"channels":[0],"data_type":"LOADCELL_50_WEIGHT_KG","labels":["Weight"],
                "scale_mode":"shared","display_filter":{"kind":"raw"}}]});
        let mut alternate = sensor.clone();
        alternate["chart_groups"][0]["display_filter"] =
            json!({"kind":"time_average","window_ms":500});
        let mut scale = sensor.clone();
        scale["chart_groups"][0]["scale_mode"] = json!("per_series");
        let result = with_grouped_views(layout(json!([
            {"id":"LOADCELL","label":"Loadcell","channels":[],"subtabs":[sensor.clone()]},
            {"id":"DAQ_ANALOG","label":"DAQ","channels":[],"subtabs":[sensor,alternate,scale]}
        ])));
        let view = overview(&result, "fill");
        assert_eq!(view.summary_items.unwrap().len(), 1);
        let graphs = view.chart_groups.unwrap();
        assert_eq!(graphs.len(), 3);
        assert_eq!(
            graphs[0].display_filter.as_ref().unwrap().kind,
            DataDisplayFilterKind::Raw
        );
        assert_eq!(
            graphs[1].display_filter.as_ref().unwrap().window_ms,
            Some(500)
        );
        assert_eq!(graphs[2].scale_mode, Some(DataChartScaleMode::PerSeries));
    }

    #[test]
    fn selected_fill_source_and_disabled_charts_are_preserved() {
        let result = with_grouped_views(layout(
            json!([{"id":"LOADCELL","label":"Loadcell","channels":[],"subtabs":[
                {"id":"kg1000","label":"1000kg","data_type":"KG1000","channels":["Raw"],"chart":{"enabled":false}},
                {"id":"kg50","label":"Selected 50kg","data_type":"KG50","channels":["Raw"],
                 "summary_items":[{"label":"Fill %","data_type":"LOADCELL_FILL_PERCENT","index":0,"show_min_max":false}],
                 "chart_groups":[{"data_type":"LOADCELL_FILL_PERCENT","channels":[0],"labels":["Fill %"]}]}
            ]}]),
        ));
        let view = overview(&result, "fill");
        let items = view.summary_items.unwrap();
        assert_eq!(
            items
                .iter()
                .filter(|i| i.data_type == "LOADCELL_FILL_PERCENT")
                .count(),
            1
        );
        assert!(
            items
                .iter()
                .find(|i| i.data_type == "LOADCELL_FILL_PERCENT")
                .unwrap()
                .label
                .starts_with("Selected 50kg")
        );
        let graphs = view.chart_groups.unwrap();
        assert_eq!(graphs.len(), 1);
        assert_eq!(
            graphs[0].chart_series.as_ref().unwrap()[0].data_type,
            "LOADCELL_FILL_PERCENT"
        );
    }

    #[test]
    fn custom_overview_id_wins_and_empty_configuration_stays_empty() {
        assert!(with_grouped_views(layout(json!([]))).tabs.is_empty());
        let input = layout(json!([
            {"id":"gs26-overview-fill", "label":"My existing view", "channels":["Custom"]},
            {"id":"KG50", "label":"50kg", "channels":["Mass"]}
        ]));
        assert_eq!(with_grouped_views(input.clone()), input);
    }

    #[test]
    fn heterogeneous_summaries_and_graphs_keep_explicit_source_indices_and_axes() {
        let input = layout(
            json!([{"id":"KALMAN_FILTER","label":"Flight","channels":[],"subtabs":[
                {"id":"ascent","label":"Ascent","show_min_max":false,
                 "summary_items":[{"label":"Altitude m","data_type":"ASCENT_STATE","index":4},
                                  {"label":"Velocity m/s","data_type":"ASCENT_STATE","index":5}],
                 "chart_groups":[{"title":"Configured separate scales","channels":[0,1],"scale_mode":"per_series"}]}
            ]}]),
        );
        let result = with_grouped_views(input);
        let view = overview(&result, "avionics");
        let groups = view.chart_groups.unwrap();
        assert_eq!(
            groups.len(),
            1,
            "keep the configured graph, invent no shared graph"
        );
        let series = groups[0].chart_series.as_ref().unwrap();
        assert_eq!(
            series.iter().map(|s| s.index).collect::<Vec<_>>(),
            vec![4, 5]
        );
        assert!(series.iter().all(|s| s.data_type == "ASCENT_STATE"));
        assert_eq!(groups[0].scale_mode, Some(DataChartScaleMode::PerSeries));
        assert!(
            view.summary_items
                .unwrap()
                .iter()
                .all(|s| s.show_min_max == Some(false))
        );
    }
}

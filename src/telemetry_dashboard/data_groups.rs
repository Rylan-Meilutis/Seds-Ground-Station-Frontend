//! UI-only overviews of the configured sensor views. No telemetry is synthesized.
use super::layout::{
    ChartSeriesSpec, DataChartGroup, DataSubtabSpec, DataSummaryItem, DataTabChart, DataTabLayout,
    DataTabSpec,
};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Fill,
    Avionics,
    Power,
}

fn family(source: &str) -> Option<Family> {
    if source.starts_with("BATTERY_")
        || source.contains("_BATTERY_")
        || source.starts_with("FILL_BOX_POWER_")
        || matches!(source, "DAQ_CURRENT_SENSE" | "DAQ_POWER_MONITOR")
    {
        Some(Family::Power)
    } else if source.starts_with("LOADCELL")
        || matches!(
            source,
            "KG50"
                | "KG1000"
                | "FUEL_TANK_PRESSURE"
                | "COMBUSTION_CHAMBER_PRESSURE"
                | "PRESSURE_TRANSDUCER_CALIBRATED"
                | "VALVE_STATE"
        )
    {
        Some(Family::Fill)
    } else if matches!(
        source,
        "GYRO_DATA"
            | "EULER_ANGLES"
            | "ACCEL_DATA"
            | "BAROMETER_DATA"
            | "BAROMETER_LOCAL"
            | "KALMAN_FILTER"
            | "ASCENT_STATE"
            | "DESCENT_STATE"
            | "GPS_DATA"
            | "VEHICLE_SPEED"
    ) {
        Some(Family::Avionics)
    } else {
        None
    }
}

struct Overview {
    family: Family,
    id: &'static str,
    label: &'static str,
    summaries: Vec<DataSummaryItem>,
    charts: Vec<DataChartGroup>,
    summary_sources: HashSet<String>,
    chart_sources: HashSet<String>,
}

impl Overview {
    fn new(family: Family, id: &'static str, label: &'static str) -> Self {
        Self {
            family,
            id,
            label,
            summaries: Vec::new(),
            charts: Vec::new(),
            summary_sources: HashSet::new(),
            chart_sources: HashSet::new(),
        }
    }

    fn add(&mut self, tab: &DataTabSpec, sub: Option<&DataSubtabSpec>, layout: &DataTabLayout) {
        let source = sub.and_then(|s| s.data_type.as_deref()).unwrap_or(&tab.id);
        let sender = sub.and_then(|s| s.sender_id.clone());
        let context = sub.map(|s| s.label.as_str()).unwrap_or(&tab.label);
        let labels = sub
            .and_then(|s| s.channels.as_ref())
            .unwrap_or(&tab.channels);
        let formatters = sub
            .and_then(|s| s.channel_formatters.as_ref())
            .or(tab.channel_formatters.as_ref());
        let booleans = sub
            .and_then(|s| s.channel_boolean_labels.as_ref())
            .or(tab.channel_boolean_labels.as_ref());
        let boolean = sub
            .and_then(|s| s.boolean_labels.as_ref())
            .or(tab.boolean_labels.as_ref());
        let min_max = sub
            .and_then(|s| s.show_min_max)
            .or(tab.show_min_max)
            .unwrap_or(true);
        let filter = sub
            .and_then(|s| s.display_filter.as_ref())
            .or(tab.display_filter.as_ref())
            .or(layout.default_display_filter.as_ref());
        let inherited_family = family(source);
        let configured = sub
            .and_then(|s| s.summary_items.as_ref())
            .filter(|items| !items.is_empty());
        let items = configured.cloned().unwrap_or_else(|| {
            labels
                .iter()
                .enumerate()
                .map(|(index, label)| DataSummaryItem {
                    label: label.clone(),
                    data_type: source.to_string(),
                    index,
                    sender_id: sender.clone(),
                    formatter: formatters.and_then(|f| f.get(index)).cloned(),
                    boolean_labels: booleans.and_then(|b| b.get(index)).or(boolean).cloned(),
                    show_min_max: Some(min_max),
                    display_filter: filter.cloned(),
                })
                .collect()
        });
        for item in &items {
            if family(&item.data_type).or(inherited_family) != Some(self.family) {
                continue;
            }
            let mut item = item.clone();
            // Explicit summary sources retain their own sender, including None (unscoped).
            item.show_min_max = Some(item.show_min_max.unwrap_or(min_max));
            item.display_filter = item.display_filter.or_else(|| filter.cloned());
            let mut key_item = item.clone();
            key_item.label.clear();
            let key = serde_json::to_string(&key_item).expect("layout metadata serializes");
            if self.summary_sources.insert(key) {
                item.label = format!("{context} · {}", item.label);
                self.summaries.push(item);
            }
        }
        if !sub
            .and_then(|s| s.chart.as_ref())
            .or(tab.chart.as_ref())
            .map(|c| c.enabled)
            .unwrap_or(true)
        {
            return;
        }
        let groups = sub
            .and_then(|s| s.chart_groups.as_ref())
            .or(tab.chart_groups.as_ref())
            .cloned()
            .unwrap_or_else(|| {
                vec![DataChartGroup {
                    title: None,
                    data_type: None,
                    sender_id: None,
                    labels: None,
                    channels: (0..labels.len().max(items.len())).collect(),
                    chart_series: None,
                    scale_mode: None,
                    display_filter: None,
                }]
            });
        for mut group in groups {
            let group_source = group.data_type.as_deref().unwrap_or(source);
            let group_sender = group.sender_id.clone().or_else(|| sender.clone());
            let legend = group.labels.as_ref().unwrap_or(labels);
            let series = group
                .chart_series
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    group
                        .channels
                        .iter()
                        .enumerate()
                        .filter_map(|(position, index)| {
                            let label = legend.get(*index).or_else(|| legend.get(position));
                            // A group without a source can address heterogeneous summary items.
                            if group.data_type.is_none() && configured.is_some() {
                                let item = label
                                    .and_then(|label| {
                                        items.iter().find(|i| i.label.eq_ignore_ascii_case(label))
                                    })
                                    .or_else(|| items.get(*index))
                                    .or_else(|| items.get(position))?;
                                Some(ChartSeriesSpec {
                                    data_type: item.data_type.clone(),
                                    index: item.index,
                                    sender_id: item.sender_id.clone(),
                                    label: Some(item.label.clone()),
                                    display_filter: item.display_filter.clone(),
                                })
                            } else {
                                Some(ChartSeriesSpec {
                                    data_type: group_source.to_string(),
                                    index: *index,
                                    sender_id: group_sender.clone(),
                                    label: label.cloned(),
                                    display_filter: None,
                                })
                            }
                        })
                        .collect()
                });
            let mut selected = Vec::new();
            for mut series in series {
                if series.data_type.is_empty()
                    || family(&series.data_type).or(inherited_family) != Some(self.family)
                {
                    continue;
                }
                series.label = Some(format!(
                    "{context} · {}",
                    series
                        .label
                        .unwrap_or_else(|| { format!("{} [{}]", series.data_type, series.index) })
                ));
                selected.push(series);
            }
            if selected.is_empty() {
                continue;
            }
            group.display_filter = group.display_filter.or_else(|| filter.cloned());
            // Keep separate configured axes/scales and filter variants. Identical graph copies
            // under LOADCELL and DAQ collapse without merging different physical units.
            let mut key_series = selected.clone();
            for series in &mut key_series {
                series.label = None;
            }
            let key =
                serde_json::to_string(&(&key_series, &group.scale_mode, &group.display_filter))
                    .expect("layout metadata serializes");
            if !self.chart_sources.insert(key) {
                continue;
            }
            group.title = Some(format!(
                "{context} · {}",
                group.title.as_deref().unwrap_or("History")
            ));
            group.data_type = Some(selected[0].data_type.clone());
            group.sender_id = selected[0].sender_id.clone();
            group.labels = Some(
                selected
                    .iter()
                    .map(|s| s.label.clone().unwrap_or_default())
                    .collect(),
            );
            group.channels = (0..selected.len()).collect();
            group.chart_series = Some(selected);
            self.charts.push(group);
        }
    }

    fn into_tab(self) -> Option<DataTabSpec> {
        if self.summaries.is_empty() && self.charts.is_empty() {
            return None;
        }
        Some(DataTabSpec {
            id: self.id.to_string(),
            label: self.label.to_string(),
            channels: Vec::new(),
            chart: None,
            chart_groups: None,
            boolean_labels: None,
            channel_boolean_labels: None,
            channel_formatters: None,
            show_min_max: None,
            display_filter: None,
            subtabs: Some(vec![DataSubtabSpec {
                id: "overview".to_string(),
                label: "Overview".to_string(),
                data_type: None,
                sender_id: None,
                channels: Some(Vec::new()),
                chart: Some(DataTabChart { enabled: true }),
                chart_groups: Some(self.charts),
                summary_items: Some(self.summaries),
                boolean_labels: None,
                channel_boolean_labels: None,
                channel_formatters: None,
                show_min_max: None,
                display_filter: None,
            }]),
        })
    }
}

/// Append overviews after fill-source selection. Existing tabs, IDs and metadata are untouched.
/// Equivalent summary copies collapse; distinct formatting and filter policies remain visible.
/// The new IDs are navigation only: all readouts and graph series name real configured sources.
pub fn with_grouped_views(mut layout: DataTabLayout) -> DataTabLayout {
    let mut overviews = [
        Overview::new(Family::Fill, "gs26-overview-fill", "Fill overview"),
        Overview::new(
            Family::Avionics,
            "gs26-overview-avionics",
            "Avionics overview",
        ),
        Overview::new(Family::Power, "gs26-overview-power", "Power overview"),
    ];
    for tab in &layout.tabs {
        if overviews.iter().any(|o| o.id == tab.id) {
            continue;
        }
        for overview in &mut overviews {
            if let Some(subtabs) = tab.subtabs.as_ref().filter(|s| !s.is_empty()) {
                for sub in subtabs {
                    overview.add(tab, Some(sub), &layout);
                }
            } else {
                overview.add(tab, None, &layout);
            }
        }
    }
    for overview in overviews {
        if !layout.tabs.iter().any(|t| t.id == overview.id)
            && let Some(tab) = overview.into_tab()
        {
            layout.tabs.push(tab);
        }
    }
    layout
}

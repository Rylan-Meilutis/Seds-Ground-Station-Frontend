//! Raw streams (docs/live-diagrams.md): every telemetry stream the station
//! decodes, with live values and rates, plus the connection details of the
//! read-only external feed (GroundStation docs/backend/raw-stream-feed.md).
//! Administrators issue and revoke "stream viewer" tokens here; a token is
//! shown once and can only read the feed.
use super::layout::ThemeConfig;
use crate::auth;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const CATALOG_PATH: &str = "/api/stream/raw/catalog";
pub(crate) const WS_PATH: &str = "/api/stream/raw/ws";
pub(crate) const NDJSON_PATH: &str = "/api/stream/raw";
pub(crate) const TOKENS_PATH: &str = "/api/admin/stream-tokens";

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Catalog {
    pub schema: String,
    pub server_time_ms: Option<i64>,
    /// `{station: "live"|"sim", igt_replay, mode}`.
    pub provenance: Option<serde_json::Value>,
    pub streams: Vec<StreamEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub(crate) struct StreamEntry {
    pub data_type: String,
    pub sender: String,
    pub rows: u64,
    pub rate_hz: Option<f64>,
    pub last_received_ms: Option<i64>,
    pub last_source_ms: Option<i64>,
    pub values: Vec<Option<f64>>,
    pub fields: Vec<String>,
    pub units: Vec<String>,
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub(crate) struct TokenSummary {
    pub id: String,
    pub label: String,
    pub issuer: Option<String>,
    pub created_ms: Option<i64>,
    pub expires_at_ms: Option<i64>,
    pub last_used_ms: Option<i64>,
    pub types: Vec<String>,
    pub senders: Vec<String>,
    pub max_hz: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
struct TokenList {
    tokens: Vec<TokenSummary>,
}

/// Accepts `{tokens:[…]}` or a bare array.
#[derive(Deserialize)]
#[serde(untagged)]
enum TokenListReply {
    Wrapped(TokenList),
    Bare(Vec<TokenSummary>),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
struct IssuedToken {
    id: String,
    token: String,
    label: String,
    expires_at_ms: Option<i64>,
}

#[derive(Serialize)]
struct IssueRequest {
    label: String,
    ttl_hours: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    types: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    senders: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_hz: Option<f64>,
}

#[derive(Serialize)]
struct Empty {}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Revoked {
    revoked: u64,
}

pub(crate) fn split_list(text: &str) -> Vec<String> {
    text.split([',', ' ', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn format_values(entry: &StreamEntry) -> String {
    entry
        .values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let value = v.map(|v| if v.fract() == 0.0 && v.abs() < 1e9 { format!("{v:.0}") } else { format!("{v:.3}") })
                .unwrap_or_else(|| "null".into());
            let unit = entry.units.get(i).filter(|u| !u.is_empty()).map(|u| format!(" {u}")).unwrap_or_default();
            match entry.fields.get(i).filter(|f| !f.is_empty()) {
                Some(field) => format!("{field} {value}{unit}"),
                None => format!("{value}{unit}"),
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// "live · Integrated ground test (FC replay running)" from the catalog.
pub(crate) fn station_provenance(catalog: &Catalog) -> Option<String> {
    let p = catalog.provenance.as_ref()?;
    let mut parts = vec![p.get("station")?.as_str()?.to_string()];
    if let Some(mode) = p.get("mode").and_then(|m| m.as_str()).filter(|m| !m.is_empty()) {
        parts.push(mode.to_string());
    }
    if p.get("igt_replay").and_then(|v| v.as_bool()) == Some(true) {
        parts.push("FC replay rows marked replay".into());
    }
    Some(parts.join(" · "))
}

/// The station origin the external feed is reached at.
fn station_origin() -> String {
    let base = super::UrlConfig::base_http();
    if !base.trim().is_empty() {
        return base.trim_end_matches('/').to_string();
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(origin) = web_sys::window().and_then(|w| w.location().origin().ok()) {
        return origin;
    }
    "https://<station>".into()
}

/// `ws(s)://…/api/stream/raw/ws?token=…&types=…` for the given filters.
pub(crate) fn feed_urls(origin: &str, types: &str, senders: &str, max_hz: &str) -> (String, String) {
    let mut query = vec!["token=<TOKEN>".to_string()];
    let types = split_list(types);
    let senders = split_list(senders);
    if !types.is_empty() {
        query.push(format!("types={}", types.join(",")));
    }
    if !senders.is_empty() {
        query.push(format!("senders={}", senders.join(",")));
    }
    if let Ok(hz) = max_hz.trim().parse::<f64>()
        && hz > 0.0
    {
        query.push(format!("max_hz={hz}"));
    }
    let query = query.join("&");
    let ws_origin = if let Some(rest) = origin.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = origin.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        origin.to_string()
    };
    (format!("{ws_origin}{WS_PATH}?{query}"), format!("{origin}{NDJSON_PATH}?{query}"))
}

fn copy_to_clipboard(text: &str) {
    let quoted = serde_json::to_string(text).unwrap_or_default();
    super::js_eval(&format!(
        "try{{navigator.clipboard&&navigator.clipboard.writeText({quoted});}}catch(e){{}}"
    ));
}

fn age_text(now_ms: i64, at: Option<i64>) -> String {
    match at {
        Some(at) => {
            let age = now_ms.saturating_sub(at).max(0);
            if age < 1_000 { format!("{age} ms") } else { super::live_diagrams::derive::format_age(age) }
        }
        None => "—".into(),
    }
}

async fn sleep_ms(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

const CSS: &str = r#"
.gs26-raw { padding:12px; display:flex; flex-direction:column; gap:14px; box-sizing:border-box; }
.gs26-raw h2 { font-size:18px; margin:0; }
.gs26-raw h3 { font-size:15px; margin:0 0 6px; }
.gs26-raw section { border:1px solid var(--raw-border); border-radius:12px; padding:12px; }
.gs26-raw input, .gs26-raw button, .gs26-raw select { font:inherit; min-height:40px; border-radius:8px; border:1px solid var(--raw-border); background:transparent; color:inherit; padding:6px 10px; box-sizing:border-box; max-width:100%; }
.gs26-raw .row { display:flex; flex-wrap:wrap; gap:8px; align-items:flex-end; }
.gs26-raw label { display:flex; flex-direction:column; gap:4px; font-size:13px; min-width:0; }
.gs26-raw code, .gs26-raw .mono { font-family:ui-monospace, SFMono-Regular, Menlo, monospace; font-size:12px; overflow-wrap:anywhere; }
.gs26-raw .copy { display:flex; gap:6px; align-items:center; }
.gs26-raw .copy input { flex:1 1 auto; min-width:0; font-family:ui-monospace, monospace; font-size:12px; }
.gs26-raw table { width:100%; border-collapse:collapse; font-size:13px; }
.gs26-raw th, .gs26-raw td { text-align:left; padding:5px 8px; border-bottom:1px solid var(--raw-border); vertical-align:top; }
.gs26-raw td.num { text-align:right; font-variant-numeric:tabular-nums; white-space:nowrap; }
.gs26-raw tr[data-stale=true] td { opacity:.55; }
.gs26-raw .note { font-size:12px; opacity:.8; margin:4px 0 0; }
.gs26-raw .token { padding:8px; border-radius:8px; border:1px dashed currentColor; }
@media (max-width:700px) { .gs26-raw th.opt, .gs26-raw td.opt { display:none; } }
"#;

#[component]
pub(crate) fn RawStreamsTab(theme: ThemeConfig) -> Element {
    let mut catalog = use_signal(|| None::<Catalog>);
    let mut error = use_signal(String::new);
    let mut filter = use_signal(String::new);
    let mut types = use_signal(String::new);
    let mut senders = use_signal(String::new);
    let mut max_hz = use_signal(|| "10".to_string());
    use_future(move || async move {
        loop {
            if super::dashboard_page_visible() {
                match super::http_get_json::<Catalog>(CATALOG_PATH).await {
                    Ok(next) => {
                        error.set(String::new());
                        catalog.set(Some(next));
                    }
                    Err(e) => error.set(format!("Raw stream catalog unavailable: {e}")),
                }
            }
            sleep_ms(1_000).await;
        }
    });
    let now = super::current_wallclock_ms();
    let cat = catalog.read().clone().unwrap_or_default();
    let needle = filter.read().to_lowercase();
    let mut streams: Vec<StreamEntry> = cat
        .streams
        .iter()
        .filter(|s| needle.is_empty() || s.data_type.to_lowercase().contains(&needle) || s.sender.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    streams.sort_by(|a, b| a.data_type.cmp(&b.data_type).then_with(|| a.sender.cmp(&b.sender)));
    let schema = if cat.schema.is_empty() { "gs26.raw-stream.v1".to_string() } else { cat.schema.clone() };
    let origin = station_origin();
    let (ws_url, ndjson_url) = feed_urls(&origin, &types.read(), &senders.read(), &max_hz.read());
    let curl = format!("curl -N -H 'Authorization: Bearer <TOKEN>' '{}'", ndjson_url.replace("token=<TOKEN>&", "").replace("?token=<TOKEN>", ""));
    rsx! {
        style { {CSS} }
        div { class: "gs26-raw", style: "--raw-border:{theme.border};color:{theme.text_primary};",
            h2 { "Raw streams" }
            p { class: "note", "Every telemetry stream the station decodes, as received. Read-only: nothing here sends a command." }
            section { "data-raw-connection": "",
                h3 { "External feed (OBS, overlays, scripts)" }
                p { class: "note", "Authenticate with a stream viewer token (issued by an administrator below) or a signed-in session. Browsers and OBS pass the token in the URL; scripts should send it as a Bearer header." }
                div { class: "row",
                    label { "Types (comma list, prefix*)" input { value: "{types}", placeholder: "all", oninput: move |e| types.set(e.value()) } }
                    label { "Senders" input { value: "{senders}", placeholder: "all", oninput: move |e| senders.set(e.value()) } }
                    label { "Max Hz per stream" input { value: "{max_hz}", inputmode: "decimal", oninput: move |e| max_hz.set(e.value()) } }
                }
                for (label, value) in [("WebSocket", ws_url.clone()), ("NDJSON stream", ndjson_url.clone()), ("curl", curl.clone())] {
                    label { key: "{label}", "{label}"
                        div { class: "copy",
                            input { readonly: true, value: "{value}", "aria-label": "{label} URL" }
                            button { r#type: "button", onclick: { let v = value.clone(); move |_| copy_to_clipboard(&v) }, "Copy" }
                        }
                    }
                }
                p { class: "note", "Schema {schema}: rows carry data type, sender, receipt/source/network timestamps, values, fields, units and provenance (live, sim or replay). See GroundStation docs/backend/raw-stream-feed.md." }
            }
            if auth::is_admin() {
                StreamTokenAdmin { theme: theme.clone() }
            }
            section {
                div { class: "row", style: "justify-content:space-between;align-items:center;",
                    h3 { "Streams ({streams.len()})" }
                    input { value: "{filter}", placeholder: "Filter type or sender", "aria-label": "Filter streams", oninput: move |e| filter.set(e.value()) }
                }
                if !error.read().is_empty() { p { role: "status", "{error}" } }
                if let Some(provenance) = station_provenance(&cat) { p { class: "note", "Station data: {provenance}" } }
                table {
                    thead { tr { th { "Type" } th { "Sender" } th { "Latest values" } th { class: "num", "Rate" } th { class: "num", "Age" } th { class: "opt", "Provenance" } th { class: "num opt", "Rows" } } }
                    tbody {
                        for s in streams.iter() {
                            tr { key: "{s.data_type}/{s.sender}", "data-stale": (s.last_received_ms.is_none_or(|t| now - t > 10_000)).to_string(),
                                td { class: "mono", "{s.data_type}" }
                                td { "{s.sender}" }
                                td { class: "mono", "{format_values(s)}" }
                                td { class: "num", {s.rate_hz.map(|r| format!("{r:.1} Hz")).unwrap_or_else(|| "—".into())} }
                                td { class: "num", "{age_text(now, s.last_received_ms)}" }
                                td { class: "opt", {s.provenance.clone().unwrap_or_default()} }
                                td { class: "num opt", "{s.rows}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn StreamTokenAdmin(theme: ThemeConfig) -> Element {
    let mut tokens = use_signal(Vec::<TokenSummary>::new);
    let mut issued = use_signal(|| None::<IssuedToken>);
    let mut message = use_signal(String::new);
    let mut label = use_signal(|| "OBS overlay".to_string());
    let mut ttl = use_signal(|| "720".to_string());
    let mut scope_types = use_signal(String::new);
    let mut scope_senders = use_signal(String::new);
    let mut scope_hz = use_signal(String::new);
    let mut refresh = use_signal(|| 0u64);
    use_effect(move || {
        let _ = *refresh.read();
        spawn(async move {
            match super::http_get_json::<TokenListReply>(TOKENS_PATH).await {
                Ok(TokenListReply::Wrapped(list)) => tokens.set(list.tokens),
                Ok(TokenListReply::Bare(list)) => tokens.set(list),
                Err(e) => message.set(format!("Could not list stream tokens: {e}")),
            }
        });
    });
    let now = super::current_wallclock_ms();
    rsx! {
        section { "data-stream-token-admin": "",
            h3 { "Stream viewer tokens" }
            p { class: "note", "Read-only: a stream viewer token reaches only the raw feed and its catalog. It cannot send commands, open the dashboard or read any other page. The token is shown once." }
            div { class: "row",
                label { "Label" input { value: "{label}", maxlength: "64", oninput: move |e| label.set(e.value()) } }
                label { "Valid (hours)" input { value: "{ttl}", inputmode: "numeric", oninput: move |e| ttl.set(e.value()) } }
                label { "Only types (optional)" input { value: "{scope_types}", oninput: move |e| scope_types.set(e.value()) } }
                label { "Only senders (optional)" input { value: "{scope_senders}", oninput: move |e| scope_senders.set(e.value()) } }
                label { "Max Hz (optional)" input { value: "{scope_hz}", inputmode: "decimal", oninput: move |e| scope_hz.set(e.value()) } }
                button { r#type: "button", onclick: move |_| {
                    let Ok(ttl_hours) = ttl.peek().trim().parse::<u32>() else { message.set("Valid hours must be a whole number (1–8760).".into()); return; };
                    let request = IssueRequest {
                        label: label.peek().trim().to_string(),
                        ttl_hours,
                        types: split_list(&scope_types.peek()),
                        senders: split_list(&scope_senders.peek()),
                        max_hz: scope_hz.peek().trim().parse::<f64>().ok().filter(|v| *v > 0.0),
                    };
                    spawn(async move {
                        match super::http_post_json::<IssueRequest, IssuedToken>(TOKENS_PATH, &request).await {
                            Ok(token) => { message.set(String::new()); issued.set(Some(token)); let n = *refresh.peek() + 1; refresh.set(n); }
                            Err(e) => message.set(format!("Could not issue a token: {e}")),
                        }
                    });
                }, "Issue token" }
            }
            if let Some(token) = issued.read().clone() {
                div { class: "token", role: "status",
                    p { "Token for “{token.label}” (copy it now; it is not shown again):" }
                    div { class: "copy",
                        input { readonly: true, value: "{token.token}", "aria-label": "New stream viewer token" }
                        button { r#type: "button", onclick: { let t = token.token.clone(); move |_| copy_to_clipboard(&t) }, "Copy" }
                        button { r#type: "button", onclick: move |_| issued.set(None), "Done" }
                    }
                }
            }
            if !message.read().is_empty() { p { role: "status", "{message}" } }
            table {
                thead { tr { th { "Label" } th { "Scope" } th { class: "opt", "Issued by" } th { "Expires" } th { class: "opt", "Last used" } th {} } }
                tbody {
                    for t in tokens.read().iter() {
                        tr { key: "{t.id}",
                            td { "{t.label}" }
                            td { class: "mono", {
                                let mut scope = Vec::new();
                                if !t.types.is_empty() { scope.push(format!("types {}", t.types.join(","))); }
                                if !t.senders.is_empty() { scope.push(format!("senders {}", t.senders.join(","))); }
                                if let Some(hz) = t.max_hz { scope.push(format!("≤ {hz} Hz")); }
                                if scope.is_empty() { "all streams".to_string() } else { scope.join(" · ") }
                            } }
                            td { class: "opt", {t.issuer.clone().unwrap_or_default()} }
                            td { {t.expires_at_ms.map(|e| if e <= now { "expired".to_string() } else { format!("in {}", super::live_diagrams::derive::format_age(e - now)) }).unwrap_or_default()} }
                            td { class: "opt", "{age_text(now, t.last_used_ms)}" }
                            td { button { r#type: "button", onclick: { let id = t.id.clone(); move |_| {
                                let path = format!("{TOKENS_PATH}/{id}/revoke");
                                spawn(async move {
                                    match super::http_post_json::<Empty, Revoked>(&path, &Empty {}).await {
                                        Ok(r) => { message.set(format!("Revoked {} token(s); open feeds using it close within seconds.", r.revoked)); let n = *refresh.peek() + 1; refresh.set(n); }
                                        Err(e) => message.set(format!("Could not revoke: {e}")),
                                    }
                                });
                            } }, "Revoke" } }
                        }
                    }
                }
            }
            if tokens.read().is_empty() { p { class: "note", "No stream viewer tokens." } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_urls_carry_filters_and_switch_to_websocket_schemes() {
        let (ws, nd) = feed_urls("https://gs.local", "KG*, VALVE_STATE", "DAQ VB", "10");
        assert_eq!(ws, "wss://gs.local/api/stream/raw/ws?token=<TOKEN>&types=KG*,VALVE_STATE&senders=DAQ,VB&max_hz=10");
        assert_eq!(nd, "https://gs.local/api/stream/raw?token=<TOKEN>&types=KG*,VALVE_STATE&senders=DAQ,VB&max_hz=10");
        let (ws, _) = feed_urls("http://10.0.0.2:3000", "", "", "");
        assert_eq!(ws, "ws://10.0.0.2:3000/api/stream/raw/ws?token=<TOKEN>");
    }

    #[test]
    fn catalog_values_render_with_fields_and_units() {
        let catalog: Catalog = serde_json::from_value(serde_json::json!({
            "schema": "gs26.raw-stream.v1", "server_time_ms": 1, "provenance": {"station":"live","igt_replay":true,"mode":"IGT"},
            "streams": [{"data_type":"BAROMETER_DATA","sender":"FC","rows":12,"rate_hz":5.0,
                "last_received_ms":1,"last_source_ms":null,"values":[812.5,null,21.0],
                "fields":["altitude","pressure","temperature"],"units":["m","","°C"],"provenance":"live"}]
        })).unwrap();
        assert_eq!(format_values(&catalog.streams[0]), "altitude 812.500 m · pressure null · temperature 21 °C");
        assert_eq!(station_provenance(&catalog).as_deref(), Some("live · IGT · FC replay rows marked replay"));
    }
}

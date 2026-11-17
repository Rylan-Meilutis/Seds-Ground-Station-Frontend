//! Shared administrator-saved locations. Two explicit ways to fill a row:
//! the station's own live GPS (the same `/api/antenna/status` fix the map
//! shows) or the location of the device running this page. Either only fills
//! the draft. Nothing is stored until Save.
use super::gps::LocationFailure;
use super::http_get_json;
use super::station_fix::{STATUS_PATH, Snapshot as StationSnapshot, StationFix, format_age as fix_age};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// Upper bound on one device-location request, including any permission prompt.
/// The browser's own timeouts (15 s precise, then 10 s coarse) do not count
/// time spent waiting on the prompt.
const LOCATION_DEADLINE_MS: u64 = 45_000;
/// Matches the map's station marker refresh.
const STATION_POLL_MS: u64 = 3_000;
/// The dashboard's own location listener counts as this device's location
/// while its latest reading is at most this old.
#[cfg(target_os = "android")]
const RECENT_DEVICE_FIX_MS: u64 = 120_000;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
const NATIVE_LOCATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const STATIONS: [&str; 2] = ["GroundStation", "RRLS"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Coordinate {
    latitude: f64,
    longitude: f64,
    altitude_m: Option<f64>,
    accuracy_m: Option<f64>,
    source: String,
}
#[derive(Clone, Default, Deserialize)]
struct Document {
    revision: u64,
    ground_station: Option<Coordinate>,
    rrls: Option<Coordinate>,
    #[serde(default)]
    updated_at_ms: Option<i64>,
    #[serde(default)]
    updated_by: Option<String>,
}
impl Document {
    fn rows(&self) -> [Option<Coordinate>; 2] {
        [self.ground_station.clone(), self.rrls.clone()]
    }
}
#[derive(Serialize)]
struct Update {
    expected_revision: u64,
    ground_station: Option<Coordinate>,
    rrls: Option<Coordinate>,
}
#[derive(Default, Deserialize)]
struct LocationResult {
    coordinate: Option<Coordinate>,
    /// How old the reading was when it was returned, where known.
    #[serde(default)]
    age_ms: Option<u64>,
    /// Browser/WebView failures arrive as a code; the wording lives here.
    #[serde(default)]
    failure: Option<LocationFailure>,
    /// Native location services report their own wording.
    #[serde(default)]
    error: Option<String>,
}
impl LocationResult {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            error: Some(message.into()),
            ..Self::default()
        }
    }
    #[cfg_attr(not(any(target_arch = "wasm32", target_os = "android", test)), allow(dead_code))]
    fn failure(failure: LocationFailure) -> Self {
        Self {
            failure: Some(failure),
            ..Self::default()
        }
    }
    fn error_message(&self) -> String {
        self.error
            .clone()
            .or_else(|| self.failure.map(|f| f.message().to_string()))
            .unwrap_or_else(|| "This device did not provide valid coordinates.".into())
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl From<super::gps::DeviceFix> for Coordinate {
    fn from(fix: super::gps::DeviceFix) -> Self {
        Self {
            latitude: fix.latitude,
            longitude: fix.longitude,
            altitude_m: fix.altitude_m,
            accuracy_m: fix.accuracy_m,
            source: "device_location".into(),
        }
    }
}
#[derive(Clone, Default, PartialEq)]
struct Form {
    latitude: String,
    longitude: String,
    altitude: String,
    accuracy: Option<f64>,
    source: String,
    /// Edited since the last load or save.
    dirty: bool,
}

/// Accepts `43.5`, `43,5` (decimal comma), a trailing degree sign and a
/// typographic minus. Grouping separators are rejected rather than guessed.
fn parse_decimal(text: &str) -> Option<f64> {
    let text = text.trim().trim_end_matches('°').trim().replace('\u{2212}', "-");
    if text.is_empty() || !text.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+')) {
        return None;
    }
    let text = if !text.contains('.') && text.matches(',').count() == 1 {
        text.replace(',', ".")
    } else {
        text
    };
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

impl Form {
    fn from_coordinate(p: Option<Coordinate>) -> Self {
        match p {
            Some(p) => Self {
                latitude: p.latitude.to_string(),
                longitude: p.longitude.to_string(),
                altitude: p.altitude_m.map(|a| a.to_string()).unwrap_or_default(),
                accuracy: p.accuracy_m,
                source: p.source,
                dirty: false,
            },
            None => Self::default(),
        }
    }
    fn is_empty(&self) -> bool {
        self.latitude.trim().is_empty()
            && self.longitude.trim().is_empty()
            && self.altitude.trim().is_empty()
    }
    /// Any typed edit makes the row a manual entry; a device accuracy no longer applies.
    fn edited(&mut self) {
        self.source = "manual".into();
        self.accuracy = None;
        self.dirty = true;
    }
    fn coordinate(&self, station: &str) -> Result<Option<Coordinate>, String> {
        if self.is_empty() {
            return Ok(None);
        }
        if self.latitude.trim().is_empty() || self.longitude.trim().is_empty() {
            return Err(format!(
                "{station}: enter both latitude and longitude, or Clear the position."
            ));
        }
        let latitude = parse_decimal(&self.latitude)
            .ok_or_else(|| format!("{station}: latitude is not a number."))?;
        let longitude = parse_decimal(&self.longitude)
            .ok_or_else(|| format!("{station}: longitude is not a number."))?;
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            return Err(format!(
                "{station}: latitude must be within ±90° and longitude within ±180°."
            ));
        }
        let altitude_m = if self.altitude.trim().is_empty() {
            None
        } else {
            Some(
                parse_decimal(&self.altitude)
                    .filter(|v| (-15_000.0..=100_000.0).contains(v))
                    .ok_or_else(|| {
                        format!("{station}: altitude must be a number of metres or left blank.")
                    })?,
            )
        };
        let measured = matches!(self.source.as_str(), "device_location" | "station_gps");
        Ok(Some(Coordinate {
            latitude,
            longitude,
            altitude_m,
            accuracy_m: if measured {
                self.accuracy.filter(|v| v.is_finite() && *v >= 0.0)
            } else {
                None
            },
            source: if measured { self.source.as_str() } else { "manual" }.into(),
        }))
    }
    fn describe(&self) -> String {
        let measured = |what: &str| match self.accuracy {
            Some(a) => format!("From {what}, ±{}", format_accuracy(a)),
            None => format!("From {what}, accuracy not reported"),
        };
        let mut text = if self.is_empty() {
            "Not set".to_string()
        } else if self.source == "device_location" {
            measured("device location")
        } else if self.source == "station_gps" {
            measured("station GPS")
        } else {
            "Entered manually".into()
        };
        if self.dirty {
            text.push_str(" · not saved yet");
        }
        text
    }
}
fn format_accuracy(meters: f64) -> String {
    if meters >= 1000.0 {
        format!("{:.1} km", meters / 1000.0)
    } else {
        format!("{meters:.0} m")
    }
}
fn format_age(age_ms: i64) -> String {
    let seconds = age_ms.max(0) / 1000;
    match seconds {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86_399 => format!("{} h ago", seconds / 3600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}
fn wall_now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}
async fn sleep_ms(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// Web and Android use the WebView's geolocation (secure context and a
/// permission prompt exist there); the script first reuses a recent fix from
/// the dashboard's own location watch. Android first reuses the app's native
/// location listener (the one feeding the map), since the WebView has its own,
/// separate provider that may not have a fix yet. Desktop WebViews and iOS do
/// not grant page geolocation, so those builds ask the operating system once.
async fn device_location() -> LocationResult {
    #[cfg(target_os = "android")]
    if let Some(fix) = super::gps_android::recent_fix(RECENT_DEVICE_FIX_MS) {
        return LocationResult {
            coordinate: Some(fix.into()),
            ..LocationResult::default()
        };
    }
    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    {
        document::eval(include_str!("../../assets/browser/station_location.js"))
            .recv::<LocationResult>()
            .await
            .unwrap_or_else(|_| LocationResult::failure(LocationFailure::Unavailable))
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    {
        match super::gps::one_shot_location(NATIVE_LOCATION_TIMEOUT).await {
            Ok(fix) => LocationResult {
                coordinate: Some(fix.into()),
                ..LocationResult::default()
            },
            Err(message) => LocationResult::failed(message),
        }
    }
}
async fn device_location_bounded() -> LocationResult {
    use futures_util::future::{Either, select};
    let request = std::pin::pin!(device_location());
    let deadline = std::pin::pin!(sleep_ms(LOCATION_DEADLINE_MS));
    match select(request, deadline).await {
        Either::Left((result, _)) => result,
        Either::Right(_) => LocationResult::failed(
            "No location arrived from this device in time. Answer the location prompt, use “Use live station GPS”, or enter coordinates.",
        ),
    }
}
fn valid_position(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
}
/// Fills the draft from a measured position, marking it unsaved.
fn fill_measured(form: &mut Form, coordinate: Coordinate, source: &str) {
    let mut next = Form::from_coordinate(Some(Coordinate {
        altitude_m: coordinate.altitude_m.filter(|v| v.is_finite()),
        accuracy_m: coordinate.accuracy_m.filter(|v| v.is_finite() && *v >= 0.0),
        source: source.into(),
        ..coordinate
    }));
    next.dirty = true;
    *form = next;
}
/// Applies this device's fix to the draft. Invalid coordinates never reach the
/// form. Returns the fix age (if reported) for the confirmation message.
fn apply_location(form: &mut Form, result: LocationResult) -> Result<Option<u64>, String> {
    let message = result.error_message();
    let coordinate = result
        .coordinate
        .filter(|c| valid_position(c.latitude, c.longitude))
        .ok_or(message)?;
    fill_measured(form, coordinate, "device_location");
    Ok(result.age_ms)
}

/// The station's fresh GPS fix as the map shows it, or why there is none.
/// Saved fallbacks and stale fixes are never offered as live GPS.
fn live_station_fix(fix: Option<&StationFix>, station: &str) -> Result<StationFix, String> {
    match fix {
        None => Err(format!("{station} is not reporting a GPS position.")),
        Some(f) if f.manual => Err(format!(
            "{station} has no fresh GPS fix; the map is showing its saved position."
        )),
        Some(f) if f.stale => Err(format!(
            "{station}'s last GPS fix is {} old (stale).",
            fix_age(f.age_ms)
        )),
        Some(f) if !f.usable() => Err(format!("{station} reported an invalid GPS position.")),
        Some(f) => Ok(f.clone()),
    }
}
/// Age and quality of a live station fix, e.g. "±3 m · 3-D, sea-level altitude · 2 s old".
fn station_fix_summary(fix: &StationFix) -> String {
    let accuracy = fix
        .accuracy_m
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(format_accuracy)
        .map(|a| format!("±{a}"))
        .unwrap_or_else(|| "accuracy not reported".into());
    let dimension = match (fix.altitude_m.filter(|v| v.is_finite()), fix.altitude_reference.as_deref()) {
        (None, _) => "2-D, no altitude".to_string(),
        (Some(_), Some("mean_sea_level")) => "3-D, sea-level altitude".into(),
        (Some(_), Some("ellipsoid")) => "3-D, ellipsoid height".into(),
        (Some(_), _) => "3-D, altitude datum unknown".into(),
    };
    format!(
        "{accuracy} · {dimension} · {} old",
        fix_age(fix.age_ms)
    )
}
fn apply_station_fix(form: &mut Form, fix: &StationFix) {
    fill_measured(
        form,
        Coordinate {
            latitude: fix.latitude,
            longitude: fix.longitude,
            altitude_m: fix.altitude_m,
            accuracy_m: fix.accuracy_m,
            source: String::new(),
        },
        "station_gps",
    );
}
fn station_row(snapshot: &StationSnapshot, index: usize) -> Option<&StationFix> {
    if index == 0 {
        snapshot.ground_station.as_ref()
    } else {
        snapshot.rrls.as_ref()
    }
}
fn device_copied_message(station: &str, age_ms: Option<u64>) -> String {
    let age = age_ms
        .filter(|age| *age >= 1_000)
        .map(|age| format!(" (fix {} old)", fix_age(age)))
        .unwrap_or_default();
    format!("{station} location copied from this device{age}. Save positions to share it.")
}
/// After a 409, rows the operator did not touch take the newly saved value and
/// edited rows keep their draft. Returns the edited stations whose saved value
/// also changed, so the operator knows they would replace someone else's edit.
fn merge_after_conflict(
    forms: &mut [Form; 2],
    base: &[Option<Coordinate>; 2],
    latest: &[Option<Coordinate>; 2],
) -> Vec<&'static str> {
    let mut contested = Vec::new();
    for index in 0..2 {
        if !forms[index].dirty {
            forms[index] = Form::from_coordinate(latest[index].clone());
        } else if base[index] != latest[index] {
            contested.push(STATIONS[index]);
        }
    }
    contested
}

#[component]
pub(super) fn StationPositionsAdmin() -> Element {
    let view_key = super::station_mode_observer::view_key();
    rsx! {for key in [view_key] { StationPositionsEditor {key:"{key}"} }}
}
#[component]
fn StationPositionsEditor() -> Element {
    let requests = super::station_settings_request::use_factory();
    let mut forms = use_signal(|| [Form::default(), Form::default()]);
    let mut base = use_signal(|| [None::<Coordinate>, None::<Coordinate>]);
    let mut revision = use_signal(|| 0u64);
    let mut saved_meta = use_signal(|| None::<(i64, String)>);
    let mut loaded = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut locating = use_signal(|| None::<usize>);
    let mut message = use_signal(String::new);
    // The same station positions the map polls, so the two always agree.
    let mut stations = use_signal(StationSnapshot::default);
    use_future(move || async move {
        loop {
            if super::dashboard_page_visible()
                && let Ok(next) = http_get_json::<StationSnapshot>(STATUS_PATH).await
                && *stations.peek() != next
            {
                stations.set(next);
            }
            sleep_ms(STATION_POLL_MS).await;
        }
    });
    let mut apply_document = move |d: Document| {
        forms.set([
            Form::from_coordinate(d.ground_station.clone()),
            Form::from_coordinate(d.rrls.clone()),
        ]);
        base.set(d.rows());
        revision.set(d.revision);
        saved_meta.set(
            d.updated_at_ms
                .map(|at| (at, d.updated_by.clone().unwrap_or_default())),
        );
        loaded.set(true);
    };
    use_future(move || async move {
        match http_get_json::<Document>("/api/station_positions").await {
            Ok(d) => apply_document(d),
            Err(e) => message.set(e),
        }
    });
    let saved_text = saved_meta.read().as_ref().map(|(at, by)| {
        let age = format_age(wall_now_ms() - at);
        if by.is_empty() {
            format!("Last saved {age}.")
        } else {
            format!("Last saved {age} by {by}.")
        }
    });
    rsx! {
        style {{include_str!("station_positions_admin.css")}}
        section {class:"station-positions-admin","aria-label":"Station locations",
            h2 {"Station locations"}
            p {class:"station-location-hint","Saved fallbacks for maps and antenna alignment. Fresh station GPS takes precedence. “Use live station GPS” copies the station’s own fix (the one on the map); “Use this device’s location” asks the phone or computer showing this page."}
            if let Some(text) = saved_text { p {class:"station-location-hint","{text}"} }
            div {class:"station-location-grid",
                for index in 0..2usize {
                    fieldset {disabled:*busy.read()||!*loaded.read(),
                        legend {{STATIONS[index]}}
                        label {"Latitude (°)" input {r#type:"text",inputmode:"decimal",autocomplete:"off",value:forms.read()[index].latitude.clone(),oninput:move|e|{let mut f=forms.write();f[index].latitude=e.value();f[index].edited();}}}
                        label {"Longitude (°)" input {r#type:"text",inputmode:"decimal",autocomplete:"off",value:forms.read()[index].longitude.clone(),oninput:move|e|{let mut f=forms.write();f[index].longitude=e.value();f[index].edited();}}}
                        label {"Altitude (m, optional)" input {r#type:"text",inputmode:"decimal",autocomplete:"off",value:forms.read()[index].altitude.clone(),oninput:move|e|{let mut f=forms.write();f[index].altitude=e.value();f[index].edited();}}}
                        p {class:"station-location-source","data-station":"{STATIONS[index]}",{forms.read()[index].describe()}}
                        {
                            let live=live_station_fix(station_row(&stations.read(),index),STATIONS[index]);
                            let status=match &live {Ok(fix)=>format!("Live station GPS: {}",station_fix_summary(fix)),Err(reason)=>format!("Live station GPS unavailable: {reason}")};
                            rsx!{p {class:"station-location-live","data-station":"{STATIONS[index]}","{status}"}}
                        }
                        div {class:"station-location-actions",
                            button {disabled:live_station_fix(station_row(&stations.read(),index),STATIONS[index]).is_err(),onclick:move |_|{
                                busy.set(true);
                                spawn(async move {
                                    // Re-read so the copied fix is the current one, not the last poll.
                                    if let Ok(next)=http_get_json::<StationSnapshot>(STATUS_PATH).await { stations.set(next); }
                                    let live=live_station_fix(station_row(&stations.read(),index),STATIONS[index]);
                                    message.set(match live {
                                        Ok(fix)=>{
                                            apply_station_fix(&mut forms.write()[index],&fix);
                                            format!("{} location copied from its live GPS ({}). Save positions to share it.",STATIONS[index],station_fix_summary(&fix))
                                        }
                                        Err(reason)=>reason,
                                    });
                                    busy.set(false);
                                });
                            },"Use live station GPS"}
                            button {onclick:move |_|{
                                busy.set(true);locating.set(Some(index));
                                message.set(format!("Requesting this device’s location for {}…",STATIONS[index]));
                                spawn(async move {
                                    // Dropping this task (page closed) also drops the request.
                                    let result=device_location_bounded().await;
                                    let applied={let mut f=forms.write();apply_location(&mut f[index],result)};
                                    message.set(match applied {
                                        Ok(age)=>device_copied_message(STATIONS[index],age),
                                        Err(e)=>e,
                                    });
                                    locating.set(None);busy.set(false);
                                });
                            },
                                if *locating.read()==Some(index) {"Locating this device…"} else {"Use this device’s location"}
                            }
                            button {onclick:move |_|{let mut f=forms.write();f[index]=Form{dirty:true,..Form::default()};message.set(format!("{} position cleared in draft. Save to apply.",STATIONS[index]));},"Clear"}
                        }
                    }
                }
            }
            p {role:"status","{message}"}
            div {class:"station-location-actions",
                button {disabled:*busy.read(),onclick:move |_|{busy.set(true);spawn(async move{match http_get_json::<Document>("/api/station_positions").await{
                    Ok(d)=>{apply_document(d);message.set("Saved positions loaded.".into())},Err(e)=>message.set(e)}busy.set(false);});},"Reload"}
                button {disabled:*busy.read()||!*loaded.read(),onclick:move |_|{
                    let current=forms.read().clone();
                    let positions=current[0].coordinate(STATIONS[0]).and_then(|g|current[1].coordinate(STATIONS[1]).map(|r|(g,r)));
                    let (g,r)=match positions{Ok(p)=>p,Err(e)=>{message.set(e);return;}};
                    let request=Update{expected_revision:*revision.read(),ground_station:g,rrls:r};busy.set(true);spawn(async move{
                        let result=requests.post::<Update,Document>("/api/station_positions",&request).await;
                        if !requests.current(){return;}
                        match result {
                            Ok(d)=>{apply_document(d);message.set("Station positions saved.".into())}
                            Err(e) if e.starts_with("HTTP 409")=>{
                                match http_get_json::<Document>("/api/station_positions").await {
                                    Ok(latest)=>{
                                        let contested={let mut f=forms.write();merge_after_conflict(&mut f,&base.read(),&latest.rows())};
                                        base.set(latest.rows());revision.set(latest.revision);
                                        saved_meta.set(latest.updated_at_ms.map(|at|(at,latest.updated_by.clone().unwrap_or_default())));
                                        message.set(if contested.is_empty(){
                                            "Positions were changed elsewhere. Rows you did not edit now show the newly saved values; your edits are kept. Save again to apply them.".into()
                                        }else{
                                            format!("{} was also changed elsewhere. Your draft is kept; Save again to replace it, or Reload to discard your draft.",contested.join(" and "))
                                        });
                                    }
                                    Err(_)=>message.set("Positions were changed elsewhere. Reload before saving; your draft is kept until then.".into()),
                                }
                            }
                            Err(e)=>message.set(e),
                        }
                        busy.set(false);
                    });},"Save positions"}
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn coordinate(lat: f64, lon: f64) -> Coordinate {
        Coordinate {
            latitude: lat,
            longitude: lon,
            altitude_m: None,
            accuracy_m: None,
            source: "manual".into(),
        }
    }
    #[test]
    fn coordinates_require_a_complete_valid_pair_and_clear_explicitly() {
        assert!(Form::default().coordinate("GS").unwrap().is_none());
        let mut form = Form {
            latitude: "43.0".into(),
            longitude: "-78.8".into(),
            ..Default::default()
        };
        assert_eq!(form.coordinate("GS").unwrap().unwrap().source, "manual");
        for bad in ["NaN", "inf", "91", "-90.5", "1e2", "4 3", "1,234.5", "1.234,5"] {
            form.latitude = bad.into();
            assert!(form.coordinate("GS").is_err(), "{bad}");
        }
        form.latitude = "43.0".into();
        form.longitude = String::new();
        assert!(form.coordinate("GS").unwrap_err().contains("both"));
        form.longitude = "180.1".into();
        assert!(form.coordinate("GS").is_err());
        form.longitude = "-180".into();
        form.altitude = "200000".into();
        assert!(form.coordinate("GS").unwrap_err().contains("altitude"));
    }
    #[test]
    fn decimal_commas_degree_signs_and_typographic_minus_are_accepted() {
        assert_eq!(parse_decimal(" 43,0215 "), Some(43.0215));
        assert_eq!(parse_decimal("−78.82°"), Some(-78.82));
        assert_eq!(parse_decimal("+12"), Some(12.0));
        assert_eq!(parse_decimal("1,2,3"), None);
        assert_eq!(parse_decimal(""), None);
        let form = Form {
            latitude: "43,5".into(),
            longitude: "-78,25".into(),
            altitude: "190,5".into(),
            ..Default::default()
        };
        let c = form.coordinate("RRLS").unwrap().unwrap();
        assert_eq!((c.latitude, c.longitude, c.altitude_m), (43.5, -78.25, Some(190.5)));
    }
    #[test]
    fn device_location_fills_only_the_draft_and_typing_makes_it_manual() {
        let mut form = Form::default();
        let fix = Coordinate {
            accuracy_m: Some(12.4),
            altitude_m: Some(f64::NAN),
            source: "device_location".into(),
            ..coordinate(43.02, -78.82)
        };
        apply_location(&mut form, LocationResult { coordinate: Some(fix), ..Default::default() }).unwrap();
        assert!(form.dirty);
        assert_eq!(form.describe(), "From device location, ±12 m · not saved yet");
        let saved = form.coordinate("GS").unwrap().unwrap();
        assert_eq!(saved.source, "device_location");
        assert_eq!(saved.accuracy_m, Some(12.4));
        assert_eq!(saved.altitude_m, None);
        form.latitude = "43.03".into();
        form.edited();
        let saved = form.coordinate("GS").unwrap().unwrap();
        assert_eq!((saved.source.as_str(), saved.accuracy_m), ("manual", None));
        // Errors and impossible fixes leave the draft untouched.
        let before = form.clone();
        let err = apply_location(&mut form, LocationResult::failed("Location permission was denied."));
        assert_eq!(err.unwrap_err(), "Location permission was denied.");
        let bogus = LocationResult { coordinate: Some(coordinate(95.0, 0.0)), ..Default::default() };
        assert!(apply_location(&mut form, bogus).is_err());
        assert!(form == before);
    }
    #[test]
    fn conflict_keeps_edited_rows_and_refreshes_untouched_rows() {
        let base = [Some(coordinate(43.0, -78.8)), None];
        let latest = [Some(coordinate(43.0, -78.8)), Some(coordinate(42.9, -78.7))];
        let mut forms = [Form::from_coordinate(base[0].clone()), Form::default()];
        forms[0].latitude = "43.1".into();
        forms[0].edited();
        assert!(merge_after_conflict(&mut forms, &base, &latest).is_empty());
        assert_eq!(forms[0].latitude, "43.1");
        assert_eq!(forms[1].latitude, "42.9");
        assert!(!forms[1].dirty);
        // Both sides edited the RRLS row: the draft stays and the row is reported.
        let changed = [latest[0].clone(), Some(coordinate(42.8, -78.6))];
        forms[1].longitude = "-78.5".into();
        forms[1].edited();
        assert_eq!(merge_after_conflict(&mut forms, &latest, &changed), vec!["RRLS"]);
        assert_eq!(forms[1].longitude, "-78.5");
    }
    fn live(age_ms: u64) -> StationFix {
        StationFix {
            latitude: 43.0012,
            longitude: -78.7891,
            altitude_m: Some(181.5),
            altitude_reference: Some("mean_sea_level".into()),
            accuracy_m: Some(2.6),
            age_ms,
            source: "GroundStation GPS".into(),
            ..Default::default()
        }
    }
    #[test]
    fn live_station_gps_is_offered_only_for_a_fresh_fix() {
        assert!(live_station_fix(Some(&live(800)), "GroundStation").is_ok());
        assert_eq!(
            live_station_fix(None, "RRLS").unwrap_err(),
            "RRLS is not reporting a GPS position."
        );
        let saved = StationFix { manual: true, ..live(0) };
        assert!(live_station_fix(Some(&saved), "RRLS").unwrap_err().contains("saved position"));
        let stale = StationFix { stale: true, ..live(600_000) };
        assert_eq!(
            live_station_fix(Some(&stale), "GroundStation").unwrap_err(),
            "GroundStation's last GPS fix is 10 min old (stale)."
        );
        let bogus = StationFix { latitude: f64::NAN, ..live(0) };
        assert!(live_station_fix(Some(&bogus), "GroundStation").unwrap_err().contains("invalid"));
        // Each row reads its own station from the same snapshot the map uses.
        let snapshot: StationSnapshot = serde_json::from_str(
            r#"{"rocket":null,"ground_station":{"latitude":43.0,"longitude":-78.8,"altitude_m":180.0,"altitude_reference":"mean_sea_level","accuracy_m":3.0,"geoid_separation_m":-34.0,"age_ms":1200,"stale":false,"source":"GroundStation GPS","manual":false},"rrls":null,"rssi":null,"gpsd_configured":true}"#,
        )
        .unwrap();
        assert!(live_station_fix(station_row(&snapshot, 0), STATIONS[0]).is_ok());
        assert!(live_station_fix(station_row(&snapshot, 1), STATIONS[1]).is_err());
        assert_eq!(
            station_fix_summary(station_row(&snapshot, 0).unwrap()),
            "±3 m · 3-D, sea-level altitude · 1 s old"
        );
    }
    #[test]
    fn station_gps_fills_the_draft_with_its_accuracy_and_altitude() {
        let mut form = Form::from_coordinate(Some(coordinate(1.0, 2.0)));
        apply_station_fix(&mut form, &live(1_500));
        assert!(form.dirty);
        assert_eq!((form.latitude.as_str(), form.longitude.as_str()), ("43.0012", "-78.7891"));
        assert_eq!(form.altitude, "181.5");
        assert_eq!(form.describe(), "From station GPS, ±3 m · not saved yet");
        let saved = form.coordinate("GroundStation").unwrap().unwrap();
        assert_eq!(saved.source, "station_gps");
        assert_eq!((saved.accuracy_m, saved.altitude_m), (Some(2.6), Some(181.5)));
        let flat = StationFix { altitude_m: None, accuracy_m: None, ..live(130_000) };
        assert_eq!(station_fix_summary(&flat), "accuracy not reported · 2-D, no altitude · 2 min old");
        apply_station_fix(&mut form, &flat);
        assert_eq!(form.altitude, "");
        // Typing over it makes it a manual entry again.
        form.edited();
        assert_eq!(form.coordinate("GS").unwrap().unwrap().source, "manual");
    }
    #[test]
    fn device_location_failures_explain_permission_timeout_and_unavailable_separately() {
        let decode = |json: &str| serde_json::from_str::<LocationResult>(json).unwrap().error_message();
        let denied = decode(r#"{"failure":"denied"}"#);
        let timeout = decode(r#"{"failure":"timeout"}"#);
        let unavailable = decode(r#"{"failure":"unavailable"}"#);
        assert!(denied.contains("permission"));
        assert!(timeout.contains("no location fix of its own") && timeout.contains("Use live station GPS"));
        assert!(unavailable.contains("location service is off"));
        assert!(decode(r#"{"failure":"insecure"}"#).contains("https://"));
        assert!(decode(r#"{"failure":"unsupported"}"#).contains("cannot share"));
        assert_ne!(denied, timeout);
        assert_ne!(timeout, unavailable);
        // Native services keep their own wording; nothing at all is still explained.
        assert_eq!(decode(r#"{"error":"GeoClue refused"}"#), "GeoClue refused");
        assert_eq!(decode("{}"), "This device did not provide valid coordinates.");
        let mut form = Form::default();
        assert_eq!(
            apply_location(&mut form, LocationResult::failure(LocationFailure::Timeout)).unwrap_err(),
            LocationFailure::Timeout.message()
        );
        assert!(form == Form::default());
        // A reused recent fix reports its age.
        let reused: LocationResult = serde_json::from_str(
            r#"{"coordinate":{"latitude":43.0,"longitude":-78.8,"altitude_m":null,"accuracy_m":15.0,"source":"device_location"},"age_ms":42000}"#,
        )
        .unwrap();
        let age = apply_location(&mut form, reused).unwrap();
        assert_eq!(
            device_copied_message("RRLS", age),
            "RRLS location copied from this device (fix 42 s old). Save positions to share it."
        );
        assert_eq!(
            device_copied_message("RRLS", Some(200)),
            "RRLS location copied from this device. Save positions to share it."
        );
    }
    #[test]
    fn saved_age_and_accuracy_labels_are_readable() {
        assert_eq!(format_age(5_000), "just now");
        assert_eq!(format_age(-5_000), "just now");
        assert_eq!(format_age(125_000), "2 min ago");
        assert_eq!(format_age(7_300_000), "2 h ago");
        assert_eq!(format_age(200_000_000), "2 d ago");
        assert_eq!(format_accuracy(1500.0), "1.5 km");
        assert_eq!(Form::default().describe(), "Not set");
        let manual = Form::from_coordinate(Some(coordinate(1.0, 2.0)));
        assert_eq!(manual.describe(), "Entered manually");
    }
}

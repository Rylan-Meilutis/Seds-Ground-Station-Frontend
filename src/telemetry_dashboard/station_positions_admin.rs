//! Shared administrator-saved locations; requesting device GPS is an explicit action.
//! A device location only fills the draft. Nothing is stored until Save.
use super::http_get_json;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// Upper bound on one device-location request, including any permission prompt.
/// The browser's own timeout does not count time spent waiting on the prompt.
const LOCATION_DEADLINE_MS: u64 = 30_000;
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
    error: Option<String>,
}
impl LocationResult {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            coordinate: None,
            error: Some(message.into()),
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
        let device = self.source == "device_location";
        Ok(Some(Coordinate {
            latitude,
            longitude,
            altitude_m,
            accuracy_m: if device {
                self.accuracy.filter(|v| v.is_finite() && *v >= 0.0)
            } else {
                None
            },
            source: if device { "device_location" } else { "manual" }.into(),
        }))
    }
    fn describe(&self) -> String {
        let mut text = if self.is_empty() {
            "Not set".to_string()
        } else if self.source == "device_location" {
            match self.accuracy {
                Some(a) => format!("From device location, ±{}", format_accuracy(a)),
                None => "From device location, accuracy not reported".into(),
            }
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
/// permission prompt exist there). Desktop WebViews and iOS do not grant page
/// geolocation, so those builds ask the operating system once instead.
async fn device_location() -> LocationResult {
    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    {
        document::eval(include_str!("../../assets/browser/station_location.js"))
            .recv::<LocationResult>()
            .await
            .unwrap_or_else(|_| {
                LocationResult::failed("Location is unavailable. Enter coordinates manually.")
            })
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    {
        match super::gps::one_shot_location(NATIVE_LOCATION_TIMEOUT).await {
            Ok(fix) => LocationResult {
                coordinate: Some(Coordinate {
                    latitude: fix.latitude,
                    longitude: fix.longitude,
                    altitude_m: fix.altitude_m,
                    accuracy_m: fix.accuracy_m,
                    source: "device_location".into(),
                }),
                error: None,
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
            "No location arrived in time. Answer the location prompt or enter coordinates.",
        ),
    }
}
/// Applies a device fix to the draft. Invalid coordinates never reach the form.
fn apply_location(form: &mut Form, result: LocationResult) -> Result<(), String> {
    let coordinate = result
        .coordinate
        .filter(|c| {
            c.latitude.is_finite()
                && c.longitude.is_finite()
                && (-90.0..=90.0).contains(&c.latitude)
                && (-180.0..=180.0).contains(&c.longitude)
        })
        .ok_or_else(|| {
            result
                .error
                .unwrap_or_else(|| "The device did not provide valid coordinates.".into())
        })?;
    let mut next = Form::from_coordinate(Some(Coordinate {
        altitude_m: coordinate.altitude_m.filter(|v| v.is_finite()),
        accuracy_m: coordinate.accuracy_m.filter(|v| v.is_finite() && *v >= 0.0),
        source: "device_location".into(),
        ..coordinate
    }));
    next.dirty = true;
    *form = next;
    Ok(())
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
            p {class:"station-location-hint","Saved fallbacks for maps and antenna alignment. Fresh station GPS takes precedence."}
            if let Some(text) = saved_text { p {class:"station-location-hint","{text}"} }
            div {class:"station-location-grid",
                for index in 0..2usize {
                    fieldset {disabled:*busy.read()||!*loaded.read(),
                        legend {{STATIONS[index]}}
                        label {"Latitude (°)" input {r#type:"text",inputmode:"decimal",autocomplete:"off",value:forms.read()[index].latitude.clone(),oninput:move|e|{let mut f=forms.write();f[index].latitude=e.value();f[index].edited();}}}
                        label {"Longitude (°)" input {r#type:"text",inputmode:"decimal",autocomplete:"off",value:forms.read()[index].longitude.clone(),oninput:move|e|{let mut f=forms.write();f[index].longitude=e.value();f[index].edited();}}}
                        label {"Altitude (m, optional)" input {r#type:"text",inputmode:"decimal",autocomplete:"off",value:forms.read()[index].altitude.clone(),oninput:move|e|{let mut f=forms.write();f[index].altitude=e.value();f[index].edited();}}}
                        p {class:"station-location-source","data-station":"{STATIONS[index]}",{forms.read()[index].describe()}}
                        div {class:"station-location-actions",
                            button {onclick:move |_|{
                                busy.set(true);locating.set(Some(index));
                                message.set(format!("Requesting this device’s location for {}…",STATIONS[index]));
                                spawn(async move {
                                    // Dropping this task (page closed) also drops the request.
                                    let result=device_location_bounded().await;
                                    let applied={let mut f=forms.write();apply_location(&mut f[index],result)};
                                    message.set(match applied {
                                        Ok(())=>format!("{} location copied from this device. Save positions to share it.",STATIONS[index]),
                                        Err(e)=>e,
                                    });
                                    locating.set(None);busy.set(false);
                                });
                            },
                                if *locating.read()==Some(index) {"Locating…"} else {"Use this device’s location"}
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
        apply_location(&mut form, LocationResult { coordinate: Some(fix), error: None }).unwrap();
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
        let bogus = LocationResult { coordinate: Some(coordinate(95.0, 0.0)), error: None };
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

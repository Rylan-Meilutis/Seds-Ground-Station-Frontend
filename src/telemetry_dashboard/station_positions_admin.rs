//! Shared administrator-saved locations; requesting device GPS is an explicit action.
use super::{http_get_json, http_post_json};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Serialize, Deserialize)]
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
#[derive(Clone, Default)]
struct Form {
    latitude: String,
    longitude: String,
    altitude: String,
    accuracy: Option<f64>,
    source: String,
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
            },
            None => Self::default(),
        }
    }
    fn coordinate(&self) -> Result<Option<Coordinate>, String> {
        if self.latitude.trim().is_empty()
            && self.longitude.trim().is_empty()
            && self.altitude.trim().is_empty()
        {
            return Ok(None);
        }
        let value = |s: &str| {
            s.trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| "Enter valid latitude and longitude.".to_string())
        };
        let latitude = value(&self.latitude)?;
        let longitude = value(&self.longitude)?;
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            return Err("Latitude must be within ±90° and longitude within ±180°.".into());
        }
        let altitude_m = if self.altitude.trim().is_empty() {
            None
        } else {
            Some(value(&self.altitude)?)
        };
        Ok(Some(Coordinate {
            latitude,
            longitude,
            altitude_m,
            accuracy_m: self.accuracy,
            source: if self.source == "device_location" {
                "device_location"
            } else {
                "manual"
            }
            .into(),
        }))
    }
}
#[component]
pub(super) fn StationPositionsAdmin() -> Element {
    let mut forms = use_signal(|| [Form::default(), Form::default()]);
    let mut revision = use_signal(|| 0u64);
    let mut loaded = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    use_future(move || async move {
        match http_get_json::<Document>("/api/station_positions").await {
            Ok(d) => {
                forms.set([
                    Form::from_coordinate(d.ground_station),
                    Form::from_coordinate(d.rrls),
                ]);
                revision.set(d.revision);
                loaded.set(true)
            }
            Err(e) => message.set(e),
        }
    });
    rsx! {
        style {{include_str!("station_positions_admin.css")}}
        section {class:"station-positions-admin","aria-label":"Station locations",
            h2 {"Station locations"}
            p {class:"station-location-hint","Saved fallbacks for maps and antenna alignment. Fresh station GPS takes precedence."}
            div {class:"station-location-grid",
                for index in 0..2usize {
                    fieldset {disabled:*busy.read()||!*loaded.read(),
                        legend {if index==0{"GroundStation"}else{"RRLS"}}
                        label {"Latitude (°)" input {r#type:"text",inputmode:"decimal",value:forms.read()[index].latitude.clone(),oninput:move|e|{let mut f=forms.write();f[index].latitude=e.value();f[index].source="manual".into();f[index].accuracy=None;}}}
                        label {"Longitude (°)" input {r#type:"text",inputmode:"decimal",value:forms.read()[index].longitude.clone(),oninput:move|e|{let mut f=forms.write();f[index].longitude=e.value();f[index].source="manual".into();f[index].accuracy=None;}}}
                        label {"Altitude (m, optional)" input {r#type:"text",inputmode:"decimal",value:forms.read()[index].altitude.clone(),oninput:move|e|{let mut f=forms.write();f[index].altitude=e.value();f[index].source="manual".into();}}}
                        div {class:"station-location-actions",
                            button {onclick:move |_|{busy.set(true);message.set("Requesting this device’s location…".into());spawn(async move {
                                match document::eval(include_str!("../../assets/browser/station_location.js")).recv::<LocationResult>().await {
                                    Ok(result)=>{if let Some(p)=result.coordinate{forms.write()[index]=Form::from_coordinate(Some(p));message.set("Location copied. Save positions to share it.".into());}else{message.set(result.error.unwrap_or_else(||"Location unavailable.".into()));}},
                                    Err(_)=>message.set("Location is unavailable. Enter coordinates manually.".into()),
                                }busy.set(false);
                            });},"Use this device’s location"}
                            button {onclick:move |_|{forms.write()[index]=Form::default();message.set("Position cleared in draft. Save to apply.".into());},"Clear"}
                        }
                    }
                }
            }
            p {role:"status","{message}"}
            div {class:"station-location-actions",
                button {disabled:*busy.read(),onclick:move |_|{busy.set(true);spawn(async move{match http_get_json::<Document>("/api/station_positions").await{
                    Ok(d)=>{forms.set([Form::from_coordinate(d.ground_station),Form::from_coordinate(d.rrls)]);revision.set(d.revision);loaded.set(true);message.set("Saved positions loaded.".into())},Err(e)=>message.set(e)}busy.set(false);});},"Reload"}
                button {disabled:*busy.read()||!*loaded.read(),onclick:move |_|{
                    let current=forms.read().clone();let positions=current[0].coordinate().and_then(|g|current[1].coordinate().map(|r|(g,r)));
                    let (g,r)=match positions{Ok(p)=>p,Err(e)=>{message.set(e);return;}};
                    let request=Update{expected_revision:*revision.read(),ground_station:g,rrls:r};busy.set(true);spawn(async move{
                        match http_post_json::<Update,Document>("/api/station_positions",&request).await {
                            Ok(d)=>{forms.set([Form::from_coordinate(d.ground_station),Form::from_coordinate(d.rrls)]);revision.set(d.revision);message.set("Station positions saved.".into())},Err(e)=>message.set(e)}busy.set(false);
                    });},"Save positions"}
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coordinates_require_a_complete_valid_pair_and_clear_explicitly() {
        assert!(Form::default().coordinate().unwrap().is_none());
        let mut form = Form {
            latitude: "43.0".into(),
            longitude: "-78.8".into(),
            ..Default::default()
        };
        assert_eq!(form.coordinate().unwrap().unwrap().source, "manual");
        form.latitude = "NaN".into();
        assert!(form.coordinate().is_err());
        form.latitude = "91".into();
        assert!(form.coordinate().is_err());
    }
}

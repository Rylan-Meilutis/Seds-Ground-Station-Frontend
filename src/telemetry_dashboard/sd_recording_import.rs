//! Explicit saved-file recovery. Uploads never address board transports.
use super::{UrlConfig, http_get_json};
use dioxus::prelude::*;
use serde::Deserialize;
#[derive(Clone, Deserialize)]
struct Source {
    filename: String,
    file_sha256: String,
    board_id: String,
    boot_id: String,
    bytes: u64,
}
#[derive(Deserialize)]
struct SourcePage {
    sources: Vec<Source>,
    has_more: bool,
    next_id: i64,
}
static UPLOAD_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[cfg(target_arch = "wasm32")]
async fn save_original(path: &str, name: &str) -> Result<String, String> {
    let url = format!("{}{}", UrlConfig::base_http().trim_end_matches('/'), path);
    let script = format!(
        "{}\nreturn await GSSDImport.saveSource({},{},{});",
        include_str!("../../station_ui/sd-recording-upload.js"),
        serde_json::to_string(&url).unwrap(),
        serde_json::to_string(&crate::auth::current_token()).unwrap(),
        serde_json::to_string(name).unwrap()
    );
    document::eval(&script)
        .join::<String>()
        .await
        .map_err(|e| e.to_string())
}
#[cfg(not(target_arch = "wasm32"))]
async fn save_original(path: &str, name: &str) -> Result<String, String> {
    super::recording_download::download_csv_path(path, name).await
}

#[component]
pub(super) fn SDRecordingImport(recording: String, onimport: EventHandler<String>) -> Element {
    let input_id = use_hook(|| UPLOAD_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let mut filename = use_signal(String::new);
    let mut manifest = use_signal(String::new);
    let mut board = use_signal(String::new);
    let mut boot = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut status = use_signal(String::new);
    let mut sources = use_signal(Vec::<Source>::new);
    let mut more = use_signal(|| false);
    let mut next = use_signal(|| 0i64);
    let selected = recording.clone();
    let import_recording = recording.clone();
    let download_recording = recording.clone();
    let page_recording = recording.clone();
    use_effect(move || {
        let id = selected.clone();
        spawn(async move {
            if let Ok(page) =
                http_get_json::<SourcePage>(&format!("/api/recordings/{id}/sd/sources")).await
            {
                sources.set(page.sources);
                more.set(page.has_more);
                next.set(page.next_id);
            }
        });
    });
    rsx! {details{style:"padding:12px;border:1px solid #64748b;border-radius:8px;margin:12px 0;",
        summary{"Original FC / DAQ SD sources and recording recovery"}
        if crate::auth::is_admin(){
        p{"Select a closed network recording, then stream one uncompressed firmware CSV and its recorded configuration/assignment manifest. Recovery creates a new recording; the original remains selectable."}
        label{"SD CSV " input{id:"sd-source-{input_id}",r#type:"file",accept:".csv,.CSV",disabled:busy(),onchange:move|event|{
            let Some(file)=event.files().first().cloned()else{return};
            filename.set(file.name());status.set("CSV selected. The browser will stream this file directly to disk; it is not read into an application buffer.".into());
        }}}
        p{"{filename}"}
        label{"Recorded configuration + assignment JSON " input{r#type:"file",accept:".json",disabled:busy(),onchange:move|event|{
            let Some(file)=event.files().first().cloned()else{return};
            spawn(async move{
                if file.size()>1024*1024{status.set("Manifest exceeds 1 MiB.".into());return}
                match file.read_bytes().await{Ok(bytes)=>match serde_json::from_slice::<serde_json::Value>(&bytes){Ok(value)=>{board.set(value["board_id"].as_str().unwrap_or("").into());boot.set(value["boot_id"].as_str().unwrap_or("").into());manifest.set(serde_json::to_string_pretty(&value).unwrap_or_default());},Err(error)=>status.set(format!("Invalid manifest JSON: {error}"))},Err(error)=>status.set(error.to_string())}
            });
        }}}
        label{"Physical board identity " input{value:"{board}",disabled:busy(),oninput:move|e|board.set(e.value())}}
        label{"Verified boot identity " input{value:"{boot}",disabled:busy(),oninput:move|e|boot.set(e.value())}}
        details{summary{"Review source mapping, UTC coverage and configuration"}
            p{"The manifest must identify the versioned firmware mapping, launch session, selected sensors, recorded schema, equivalent units/axes/calibration, and verified UTC assignment. Local clocks and ambiguous sessions remain in raw provenance; they do not replace UTC samples. Receipt time never establishes a match."}
            textarea{aria_label:"SD import assignment manifest",rows:16,style:"width:100%;font-family:monospace;",value:"{manifest}",disabled:busy(),oninput:move|e|manifest.set(e.value())}
        }
        button{disabled:busy()||recording.is_empty()||filename.read().is_empty()||board.read().trim().is_empty()||boot.read().trim().is_empty(),onclick:move|_|{
            let target=import_recording.clone();let source_name=filename();let board_id=board();let boot_id=boot();
            let parsed=serde_json::from_str::<serde_json::Value>(&manifest());
            let mut metadata=match parsed{Ok(serde_json::Value::Object(map))=>serde_json::Value::Object(map),_=>{status.set("Load or enter a valid assignment manifest.".into());return}};
            metadata["board_id"]=board_id.into();metadata["boot_id"]=boot_id.into();
            busy.set(true);status.set("Importing and resolving saved samples…".into());
            spawn(async move{
                let base=UrlConfig::base_http();let url=format!("{}/api/recordings/{target}/sd/imports",base.trim_end_matches('/'));
                let metadata=serde_json::json!({"manifest":metadata,"filename":source_name});
                let script=format!("{}\nreturn await GSSDImport.upload(document.getElementById('sd-source-{input_id}'),{url},{metadata},{token});",include_str!("../../station_ui/sd-recording-upload.js"),token=serde_json::to_string(&crate::auth::current_token()).unwrap(),url=serde_json::to_string(&url).unwrap(),metadata=serde_json::to_string(&metadata).unwrap());
                let result=document::eval(&script).join::<String>().await.map_err(|e|e.to_string()).and_then(|text|serde_json::from_str::<serde_json::Value>(&text).map_err(|e|e.to_string()));
                match result{Ok(result)=>{if let Some(id)=result["recording"].as_str(){onimport.call(id.to_owned());status.set(format!("Recovered recording selected: {id}. {} rows; {} network rows superseded in this resolved view. Original network and byte-exact SD source remain available. {} unresolved samples retained in provenance.",result["summary"]["rows"],result["summary"]["network_suppressed_in_resolved_view"],result["summary"]["unresolved_samples"]));}},Err(error)=>status.set(format!("SD recovery failed or canceled: {error}"))}
                busy.set(false);
            });
        },if busy(){"Recovering…"}else{"Create recovered recording"}}
        }
        if !sources.read().is_empty(){h4{"Original SD sources (all samples, including unresolved clocks)"}
            for source in sources(){
                div{p{"{source.filename} · {source.board_id} · boot {source.boot_id} · {source.bytes} bytes"}
                    button{disabled:busy(),onclick:{let path=format!("/api/recordings/{download_recording}/sd/sources/{}",source.file_sha256);let name=source.filename.clone();move|_|{let path=path.clone();let name=name.clone();spawn(async move{status.set(save_original(&path,&name).await.unwrap_or_else(|e|format!("Original SD export: {e}")));});}},"Save original SD CSV"}
                }
            }
            if more(){button{onclick:{let selected=page_recording.clone();move|_|{let id=selected.clone();spawn(async move{if let Ok(page)=http_get_json::<SourcePage>(&format!("/api/recordings/{id}/sd/sources?after_id={}",next())).await{sources.set(page.sources);more.set(page.has_more);next.set(page.next_id);}});}},"Next source page"}}
        }
        if busy(){button{onclick:move|_|{spawn(async move{let _=document::eval(&format!("document.getElementById('sd-source-{input_id}')?.__sdAbort?.abort();return true;")).join::<bool>().await;});},"Cancel upload / import"}}
        p{role:"status","{status}"}
    }}
}

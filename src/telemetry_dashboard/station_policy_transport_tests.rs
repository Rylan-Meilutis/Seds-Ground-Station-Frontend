use super::*;
use crate::telemetry_dashboard as dashboard;
use std::{future::Future, io::{Read, Write}, net::TcpListener, pin::Pin, rc::Rc,
    sync::{Mutex, atomic::{AtomicBool, Ordering}}, time::Duration};

pub(super) fn child(module: &str, name: &str) -> bool {
    let exact=format!("{}::{name}",module.split_once("::").unwrap().1);
    if std::env::var("GS_POLICY_TRANSPORT_CHILD").as_deref()==Ok(exact.as_str()) {return true;}
    let root=std::env::temp_dir().join(format!("gs-policy-ui-{}-{}",std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let result=std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact",&exact,"--nocapture"])
        .env("GS_POLICY_TRANSPORT_CHILD",&exact)
        .env("XDG_DATA_HOME",root.join("data")).env("XDG_CONFIG_HOME",root.join("config"))
        .current_dir(&root).output().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(result.status.success(),"{}\n{}",String::from_utf8_lossy(&result.stdout),String::from_utf8_lossy(&result.stderr));
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    false
}
pub(super) fn dom(base:&str)->VirtualDom {
    let mut dom=VirtualDom::new(||rsx!{div{}});dom.rebuild_in_place();
    setup(&dom,base);
    dom
}
fn setup(dom:&VirtualDom,base:&str) {
    dom.in_scope(ScopeId::ROOT,||{
        // Set the actual UI owner without persisting a station choice. This
        // isolated child never initializes/restores a host-scope auth store.
        *dashboard::BASE_URL.write()=base.to_owned();
        *dashboard::WS_EPOCH.write()=7;
        *dashboard::DASHBOARD_LIFE.write()=dashboard::DashboardLife::new_alive();
        // The fixture models an already established command socket BEFORE the
        // actual ModeGET captures its connection owner. A fresh policy frame
        // must not manufacture a false->true connection transition afterward.
        *dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap()=dashboard::FrontendNetworkMetrics{ws_connected:true,..Default::default()};
        auth::set_current_session(auth::StoredAuthSession{token:"owned-test-token".into(),remember_me:false,
            session:auth::SessionStatus{authenticated:true,permissions:auth::Permissions{view_data:true,send_commands:true,..Default::default()},..Default::default()}});
        *dashboard::ACTION_POLICY_SIGNAL.write()=serde_json::from_value(serde_json::json!({"key_enabled":true,"controls":[
            {"cmd":"StartFill","enabled":true,"blink":"none","actuated":null,"epoch_fence":"ordinary"},
            {"cmd":"ResetFlightState","enabled":true,"blink":"none","actuated":null,"epoch_fence":"ordinary"}
        ]})).unwrap();
    });
}
fn fresh_policy() -> serde_json::Value {
    serde_json::json!({"key_enabled":true,"software_buttons_enabled":true,"controls":[
        {"cmd":"StartFill","enabled":true,"blink":"none","actuated":null,"epoch_fence":"ordinary"},
        {"cmd":"ResetFlightState","enabled":true,"blink":"none","actuated":null,"epoch_fence":"ordinary"},
        {"cmd":"Underglow","enabled":true,"blink":"none","actuated":false,"epoch_fence":"ordinary"},
        {"cmd":"Abort","enabled":true,"blink":"none","actuated":false,"epoch_fence":"emergency_stop"}
    ]})
}
async fn policy(dom:&VirtualDom,server:&Server,epoch:&str,open:bool) {
    let mut snapshot=dashboard::station_mode_contract::tests::example_snapshot();
    snapshot.policy_epoch=Some(epoch.into()); snapshot.admissions_open=open;
    *server.mode.lock().unwrap()=serde_json::to_value(snapshot).unwrap();
    // Uses the actual transport and production owned response admission, not a seal setter.
    dashboard::incoming_epoch_native_tests::mode_get(dom).await;
    // Explicit fresh modeled producer input for THIS new epoch. Never restore
    // a readiness Signal captured before the real current-facts retirement hook.
    dashboard::incoming_epoch_native_tests::receive_action_policy(dom,Some(epoch),server.fresh_policy.lock().unwrap().clone());
    assert_eq!(server.mode_rows.lock().unwrap().last().unwrap().lines().next(),Some("GET /api/station-modes HTTP/1.1"));
}
async fn in_dom<F:Future+?Sized>(dom:&VirtualDom,mut future:Pin<&mut F>)->F::Output {
    std::future::poll_fn(|cx|dom.in_scope(ScopeId::ROOT,||future.as_mut().poll(cx))).await
}
struct Server {base:String,fresh_policy:Arc<Mutex<serde_json::Value>>,mode:Arc<Mutex<serde_json::Value>>,mode_rows:Arc<Mutex<Vec<String>>>,rows:Arc<Mutex<Vec<String>>>,stop:Arc<AtomicBool>,thread:Option<std::thread::JoinHandle<()>>}
impl Server {
    fn new()->Self {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();listener.set_nonblocking(true).unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());let rows=Arc::new(Mutex::new(Vec::new()));let stop=Arc::new(AtomicBool::new(false));
        let mode=Arc::new(Mutex::new(serde_json::to_value(dashboard::station_mode_contract::tests::example_snapshot()).unwrap()));
        let mode_rows=Arc::new(Mutex::new(Vec::new()));
        let (mode_reply,mode_record)=(mode.clone(),mode_rows.clone());
        let (record,end)=(rows.clone(),stop.clone());let thread=std::thread::spawn(move||{
            while !end.load(Ordering::Acquire) {match listener.accept(){
                Ok((mut stream,_))=>{stream.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
                    let mut bytes=Vec::new();let mut buffer=[0;2048];
                    while let Ok(n)=stream.read(&mut buffer){if n==0{break;}bytes.extend_from_slice(&buffer[..n]);
                        if let Some(at)=bytes.windows(4).position(|w|w==b"\r\n\r\n"){
                            let header=String::from_utf8_lossy(&bytes[..at]);
                            let len=header.lines().find_map(|l|l.split_once(':').filter(|(k,_)|k.eq_ignore_ascii_case("content-length")).and_then(|(_,v)|v.trim().parse::<usize>().ok())).unwrap_or(0);
                            if bytes.len()>=at+4+len{break;}
                        }
                        assert!(bytes.len()<65536);
                    }
                    let request=String::from_utf8(bytes).unwrap();
                    if request.starts_with("GET /api/station-modes HTTP/1.1\r\n") {
                        assert!(request.to_ascii_lowercase().contains("authorization: bearer "));
                        mode_record.lock().unwrap().push(request);
                        let body=serde_json::to_vec(&*mode_reply.lock().unwrap()).unwrap();
                        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();
                        stream.write_all(&body).unwrap(); continue;
                    }
                    record.lock().unwrap().push(request);
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
                },Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(2)),Err(e)=>panic!("{e}")}
            }
        });Self{base,fresh_policy:Arc::new(Mutex::new(fresh_policy())),mode,mode_rows,rows,stop,thread:Some(thread)}
    }
}
impl Drop for Server {fn drop(&mut self){self.stop.store(true,Ordering::Release);self.thread.take().unwrap().join().unwrap();}}

#[tokio::test(flavor="current_thread")]
async fn original_http_owner_before_first_poll_retires_and_fresh_epoch_is_sent() {
    if !child(module_path!(),"original_http_owner_before_first_poll_retires_and_fresh_epoch_is_sent"){return;}
    let server=Server::new();let dom=dom(&server.base);policy(&dom,&server,"epoch-1",true).await;
    let mut old=dom.in_scope(ScopeId::ROOT,||{
        let owner=dashboard::recording_download::ExportOwner::capture_ordinary_selected(||true).unwrap();
        Box::pin(dashboard::station_settings_request::post_captured::<_,serde_json::Value>("/api/gse/config",&serde_json::json!({"target":7}),Ok(owner)))
    });
    policy(&dom,&server,"epoch-2",true).await;
    assert!(in_dom(&dom,old.as_mut()).await.is_err());assert!(server.rows.lock().unwrap().is_empty());
    let mut fresh=dom.in_scope(ScopeId::ROOT,||{
        let owner=dashboard::recording_download::ExportOwner::capture_ordinary_selected(||true).unwrap();
        Box::pin(dashboard::station_settings_request::post_captured::<_,serde_json::Value>("/api/gse/config",&serde_json::json!({"target":8}),Ok(owner)))
    });
    in_dom(&dom,fresh.as_mut()).await.unwrap();let rows=server.rows.lock().unwrap();assert_eq!(rows.len(),1);
    assert!(rows[0].to_ascii_lowercase().contains("x-station-policy-epoch: epoch-2"));assert!(rows[0].contains("{\"target\":8}"));
}
#[tokio::test(flavor="current_thread")]
async fn actual_press_release_does_not_reacquire_epoch_and_fresh_press_works() {
    if !child(module_path!(),"actual_press_release_does_not_reacquire_epoch_and_fresh_press_works"){return;}
    let server=Server::new();let dom=dom(&server.base);policy(&dom,&server,"epoch-1",true).await;
    let (tx,mut rx)=tokio::sync::mpsc::unbounded_channel();
    dom.in_scope(ScopeId::ROOT,||{*dashboard::WS_SENDER.write()=Some(dashboard::WsSender{tx});dashboard::send_cmd_from_press("StartFill");});
    policy(&dom,&server,"epoch-2",true).await;
    dom.in_scope(ScopeId::ROOT,||dashboard::send_cmd_from_click("StartFill"));assert!(rx.try_recv().is_err());
    dom.in_scope(ScopeId::ROOT,||{dashboard::send_cmd_from_press("StartFill");dashboard::send_cmd_from_click("StartFill");});
    let actual=rx.try_recv().unwrap();assert!(dom.in_scope(ScopeId::ROOT,||actual.owner.is_current()));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&actual.wire).unwrap(),serde_json::json!({"cmd":"StartFill","policy_epoch":"epoch-2"}));
    assert!(rx.try_recv().is_err());
}
fn click(dom:&VirtualDom,id:dioxus::core::ElementId){
    dioxus::html::set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    let payload=dioxus::html::PlatformEventData::new(Box::new(dioxus::html::SerializedMouseData::default()));
    dom.runtime().handle_event("click",Event::new(Rc::new(payload)as Rc<dyn std::any::Any>,true),id);
}
#[tokio::test(flavor="current_thread")]
async fn actual_confirmation_retains_opening_epoch_and_fresh_confirmation_works(){
    if !child(module_path!(),"actual_confirmation_retains_opening_epoch_and_fresh_confirmation_works"){return;}
    let mut dom=VirtualDom::new(||rsx!{dashboard::command_confirmation::CommandConfirmation{}});
    let server=Server::new();setup(&dom,&server.base);
    dom.rebuild_in_place();
    policy(&dom,&server,"epoch-1",true).await;
    let(tx,mut rx)=tokio::sync::mpsc::unbounded_channel();
    dom.in_scope(ScopeId::ROOT,||{*dashboard::WS_SENDER.write()=Some(dashboard::WsSender{tx});let owner=CommandOwner::capture("ResetFlightState").unwrap();dashboard::command_confirmation::request("ResetFlightState",owner,dashboard::send_confirmed_cmd);});
    let edits=dom.render_immediate_to_vec();let id=edits.edits.iter().filter_map(|e|match e{dioxus::core::Mutation::NewEventListener{name,id}if name=="click"=>Some(*id),_=>None}).next_back().expect("actual Confirm handler");
    policy(&dom,&server,"epoch-2",true).await;click(&dom,id);assert!(rx.try_recv().is_err());
    dom.in_scope(ScopeId::ROOT,||{let owner=CommandOwner::capture("ResetFlightState").unwrap();dashboard::command_confirmation::request("ResetFlightState",owner,dashboard::send_confirmed_cmd);});
    dom.render_immediate_to_vec();click(&dom,id);let actual=rx.try_recv().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&actual.wire).unwrap()["policy_epoch"],"epoch-2");assert!(rx.try_recv().is_err());
}

#[tokio::test(flavor="current_thread")]
async fn policy_refresh_does_not_freshen_an_old_press() {
    if !child(module_path!(), "policy_refresh_does_not_freshen_an_old_press") { return; }
    let server=Server::new();let dom=dom(&server.base);policy(&dom,&server,"epoch-1",true).await;
    let(tx,mut rx)=tokio::sync::mpsc::unbounded_channel();
    dom.in_scope(ScopeId::ROOT,||{*dashboard::WS_SENDER.write()=Some(dashboard::WsSender{tx});dashboard::send_cmd_from_press("StartFill");});
    policy(&dom,&server,"epoch-2",true).await;
    dom.in_scope(ScopeId::ROOT,||{dashboard::clear_command_feedback_latches();dashboard::send_cmd_from_click("StartFill");});
    assert!(rx.try_recv().is_err(), "POLICY_REFRESH_MUST_NOT_FRESHEN_RETIRED_PRESS");
    dom.in_scope(ScopeId::ROOT,||{dashboard::send_cmd_from_press("StartFill");dashboard::send_cmd_from_click("StartFill");});
    let row=rx.try_recv().unwrap();assert_eq!(serde_json::from_str::<serde_json::Value>(&row.wire).unwrap()["policy_epoch"],"epoch-2");
    assert!(rx.try_recv().is_err());
}
#[tokio::test(flavor="current_thread")]
async fn pointer_touch_mouse_companions_keep_original_owner_and_consumed_release() {
    if !child(module_path!(), "pointer_touch_mouse_companions_keep_original_owner_and_consumed_release") { return; }
    let server=Server::new();let dom=dom(&server.base);policy(&dom,&server,"epoch-1",true).await;
    let(tx,mut rx)=tokio::sync::mpsc::unbounded_channel();
    dom.in_scope(ScopeId::ROOT,||{*dashboard::WS_SENDER.write()=Some(dashboard::WsSender{tx});dashboard::command_pointer_down("notice:1","StartFill",11,true,true,false);dashboard::command_touch_start("notice:1","StartFill",true,false);});
    policy(&dom,&server,"epoch-2",true).await;
    dom.in_scope(ScopeId::ROOT,||{
        dashboard::clear_command_feedback_latches();
        dashboard::command_mouse_down("notice:1","StartFill",false);
        dashboard::release_command_press("notice:1");dashboard::release_command_press("notice:1");
    });assert!(rx.try_recv().is_err());
    dom.in_scope(ScopeId::ROOT,||{
        dashboard::command_pointer_down("notice:1","StartFill",12,false,true,false);
        dashboard::command_mouse_down("notice:1","StartFill",false);
        dashboard::release_command_press("notice:1");dashboard::release_command_press("notice:1");
    });
    let row=rx.try_recv().unwrap();assert_eq!(serde_json::from_str::<serde_json::Value>(&row.wire).unwrap()["policy_epoch"],"epoch-2");assert!(rx.try_recv().is_err());
    // An actually new installed epoch must not inherit a prior epoch's burst debounce.
    policy(&dom,&server,"epoch-3",true).await;
    dom.in_scope(ScopeId::ROOT,||{dashboard::command_pointer_down("notice:1","StartFill",13,false,true,false);dashboard::release_command_press("notice:1");});
    let row=rx.try_recv().unwrap();assert_eq!(serde_json::from_str::<serde_json::Value>(&row.wire).unwrap()["policy_epoch"],"epoch-3");assert!(rx.try_recv().is_err());
}
#[tokio::test(flavor="current_thread")]
async fn feedback_flip_cannot_replace_original_close_choice() {
    if !child(module_path!(), "feedback_flip_cannot_replace_original_close_choice") { return; }
    let server=Server::new();let dom=dom(&server.base);policy(&dom,&server,"epoch-1",true).await;
    let(tx,mut rx)=tokio::sync::mpsc::unbounded_channel();
    dashboard::incoming_epoch_native_tests::receive_action_policy(&dom,Some("epoch-1"),serde_json::json!({"key_enabled":true,"controls":[
            {"cmd":"Nitrogen","enabled":true,"blink":"none","actuated":false,"epoch_fence":"ordinary"},
            {"cmd":"NitrogenClose","enabled":true,"blink":"none","actuated":true,"epoch_fence":"ordinary"}
        ]}));
    dom.in_scope(ScopeId::ROOT,||{
        *dashboard::WS_SENDER.write()=Some(dashboard::WsSender{tx});
        dashboard::command_pointer_down("action:Nitrogen","NitrogenClose",3,true,true,false);
        dashboard::command_touch_start("action:Nitrogen","Nitrogen",true,false);
        dashboard::command_mouse_down("action:Nitrogen","Nitrogen",false);
        dashboard::release_command_press("action:Nitrogen");
    });
    let row=rx.try_recv().unwrap();assert_eq!(serde_json::from_str::<serde_json::Value>(&row.wire).unwrap()["cmd"],"NitrogenClose");assert!(rx.try_recv().is_err());
}
#[tokio::test(flavor="current_thread")]
async fn primary_proxy_press_keeps_original_epoch_and_selected_command() {
    if !child(module_path!(), "primary_proxy_press_keeps_original_epoch_and_selected_command") { return; }
    let server=Server::new();
    *server.fresh_policy.lock().unwrap()=serde_json::json!({"key_enabled":false,"software_buttons_enabled":false,"controls":[]});
    let mut dom=dom(&server.base);
    dom.in_scope(ScopeId::ROOT,||{
        let mut session=auth::current_status();session.station_role=Some("rrls".into());
        session.command_proxy=Some(auth::CommandProxyStatus{supported:true,paired:true,connected:true,available:true,supported_commands:vec!["StartFill".into(),"CancelFill".into()],..Default::default()});
        auth::set_current_session(auth::StoredAuthSession{token:"proxy-original".into(),remember_me:false,session});
        *dashboard::ACTION_POLICY_SIGNAL.write()=dashboard::ActionPolicyMsg::default_locked();
    });policy(&dom,&server,"epoch-1",true).await;
    dom.in_scope(ScopeId::ROOT,||dashboard::command_pointer_down("primary:execution","StartFill",4,true,true,true));
    policy(&dom,&server,"epoch-2",true).await;
    dom.in_scope(ScopeId::ROOT,||{dashboard::command_mouse_down("primary:execution","CancelFill",true);dashboard::release_command_press("primary:execution");});
    assert!(server.rows.lock().unwrap().is_empty());
    dom.in_scope(ScopeId::ROOT,||{
        dashboard::command_pointer_down("primary:execution","StartFill",5,false,true,true);
        dashboard::command_mouse_down("primary:execution","CancelFill",true);dashboard::release_command_press("primary:execution");
    });
    tokio::time::timeout(Duration::from_secs(4),async {
        while server.rows.lock().unwrap().is_empty() {
            let _=tokio::time::timeout(Duration::from_millis(5),dom.wait_for_work()).await;
            dom.render_immediate_to_vec();
        }
    }).await.unwrap();
    let rows=server.rows.lock().unwrap();assert_eq!(rows.len(),1);
    assert!(rows[0].to_ascii_lowercase().contains("x-station-policy-epoch: epoch-2"));
    assert!(rows[0].to_ascii_lowercase().contains("authorization: bearer proxy-original"));
    assert!(rows[0].ends_with("\"StartFill\""));
}
#[tokio::test(flavor="current_thread")]
async fn secondary_legacy_stop_survives_an_unrelated_release() {
    if !child(module_path!(), "secondary_legacy_stop_survives_an_unrelated_release") { return; }
    let server=Server::new();let dom=dom(&server.base);
    let(tx,mut rx)=tokio::sync::mpsc::unbounded_channel();
    dom.in_scope(ScopeId::ROOT,||{
        let mut policy=dashboard::ACTION_POLICY_SIGNAL.read().clone();
        policy.controls.push(serde_json::from_value(serde_json::json!({"cmd":"Abort","enabled":true,"blink":"none","actuated":null})).unwrap());
        *dashboard::ACTION_POLICY_SIGNAL.write()=policy;*dashboard::WS_SENDER.write()=Some(dashboard::WsSender{tx});
        dashboard::command_pointer_down("action:StartFill","StartFill",1,true,true,false);
        dashboard::command_pointer_down("header:abort:mobile","Abort",2,true,false,false);
        dashboard::command_touch_start("header:abort:mobile","Abort",false,false);
        dashboard::release_command_press("action:StartFill");
        dashboard::release_command_press("header:abort:mobile");
    });
    let row=rx.try_recv().unwrap();assert_eq!(serde_json::from_str::<serde_json::Value>(&row.wire).unwrap(),serde_json::json!({"cmd":"Abort"}));assert!(rx.try_recv().is_err());
}

#[tokio::test(flavor="current_thread")]
async fn old_keyboard_click_cannot_replace_or_consume_new_stop() {
    if !child(module_path!(), "old_keyboard_click_cannot_replace_or_consume_new_stop") { return; }
    let server=Server::new();let dom=dom(&server.base);policy(&dom,&server,"epoch-1",true).await;
    let(tx,mut rx)=tokio::sync::mpsc::unbounded_channel();
    dom.in_scope(ScopeId::ROOT,||{
        *dashboard::WS_SENDER.write()=Some(dashboard::WsSender{tx});
        dashboard::command_key_down("keyboard:A","StartFill",false);
    });
    policy(&dom,&server,"epoch-2",true).await;
    dom.in_scope(ScopeId::ROOT,||{
        dashboard::command_pointer_down("header:B","Abort",22,false,true,false);
        dashboard::command_click_with_origin("keyboard:A","StartFill",dashboard::command_owner::ClickOrigin::KeyboardOrAccessible,None,false);
    });assert!(rx.try_recv().is_err());
    dom.in_scope(ScopeId::ROOT,||{
        assert_eq!(dashboard::PENDING_COMMAND_PRESS.lock().unwrap().as_ref().unwrap().source,"header:B");
        assert!(!dashboard::PENDING_COMMAND_PRESS.lock().unwrap().as_ref().unwrap().consumed);
        dashboard::command_click_with_origin("header:B","Abort",dashboard::command_owner::ClickOrigin::Pointer,Some(22),false);
    });
    let row=rx.try_recv().unwrap();assert_eq!(serde_json::from_str::<serde_json::Value>(&row.wire).unwrap(),serde_json::json!({"cmd":"Abort","policy_epoch":"epoch-2"}));
    dom.in_scope(ScopeId::ROOT,||dashboard::command_click_with_origin("keyboard:A","StartFill",dashboard::command_owner::ClickOrigin::KeyboardOrAccessible,None,false));
    assert!(rx.try_recv().is_err());
    dom.in_scope(ScopeId::ROOT,||{
        dashboard::command_pointer_down("keyboard:A","StartFill",23,false,true,false);
        dashboard::command_click_with_origin("keyboard:A","StartFill",dashboard::command_owner::ClickOrigin::KeyboardOrAccessible,None,false);
        assert!(!dashboard::PENDING_COMMAND_PRESS.lock().unwrap().as_ref().unwrap().consumed);
        dashboard::command_click_with_origin("keyboard:A","StartFill",dashboard::command_owner::ClickOrigin::Pointer,Some(23),false);
    });
    let row=rx.try_recv().unwrap();assert_eq!(serde_json::from_str::<serde_json::Value>(&row.wire).unwrap()["cmd"],"StartFill");
    dom.in_scope(ScopeId::ROOT,||dashboard::command_click_with_origin("keyboard:A","StartFill",dashboard::command_owner::ClickOrigin::KeyboardOrAccessible,None,false));
    assert!(rx.try_recv().is_err());
}
#[tokio::test(flavor="current_thread")]
async fn keyboard_tombstone_overflow_blocks_only_uncorrelated_clicks() {
    if !child(module_path!(), "keyboard_tombstone_overflow_blocks_only_uncorrelated_clicks") { return; }
    let server=Server::new();let dom=dom(&server.base);policy(&dom,&server,"epoch-1",true).await;
    let(tx,mut rx)=tokio::sync::mpsc::unbounded_channel();
    dom.in_scope(ScopeId::ROOT,||{
        *dashboard::WS_SENDER.write()=Some(dashboard::WsSender{tx});
        for i in 0..65 { dashboard::command_key_down(&format!("key:{i}"),"StartFill",false); }
        dashboard::command_pointer_down("header:stop","Abort",4,false,true,false);
        dashboard::command_click_with_origin("unseen:A","StartFill",dashboard::command_owner::ClickOrigin::KeyboardOrAccessible,None,false);
        dashboard::command_click_with_origin("header:stop","Abort",dashboard::command_owner::ClickOrigin::Pointer,Some(4),false);
    });
    let row=rx.try_recv().unwrap();assert_eq!(serde_json::from_str::<serde_json::Value>(&row.wire).unwrap()["cmd"],"Abort");
    dom.in_scope(ScopeId::ROOT,||dashboard::command_click_with_origin("unseen:A","StartFill",dashboard::command_owner::ClickOrigin::KeyboardOrAccessible,None,false));
    assert!(rx.try_recv().is_err());
    dom.in_scope(ScopeId::ROOT,||{
        dashboard::command_pointer_down("unseen:A","StartFill",5,false,true,false);
        dashboard::command_click_with_origin("unseen:A","StartFill",dashboard::command_owner::ClickOrigin::Pointer,Some(5),false);
    });
    let row=rx.try_recv().unwrap();assert_eq!(serde_json::from_str::<serde_json::Value>(&row.wire).unwrap()["cmd"],"StartFill");assert!(rx.try_recv().is_err());
}

// Actual ActionsTab event handlers and Dioxus's supported desktop serialized
// metadata. Synthetic layout/policy are inputs; there is no native-device claim.
fn native_terminal_actions() -> Element {
    use_context_provider(||Signal::new(dashboard::sequence_progress_host::StatusObservation::default()));
    let recording=use_signal(||serde_json::from_value::<dashboard::RecordingStatusMsg>(serde_json::json!({"mode":"idle","db_path":null})).unwrap());
    let layout=serde_json::from_value::<dashboard::layout::ActionsTabLayout>(serde_json::json!({
        "show_flight_setup":false,"show_fill_targets":false,"actions":[{"label":"Native terminal control","cmd":"Underglow","group":"Input control","border":"#fff","bg":"#000","fg":"#fff"},{"label":"Native Stop control","cmd":"Abort","group":"Input control","border":"#fff","bg":"#000","fg":"#fff"}]
    })).unwrap();
    rsx!{dashboard::ActionsTab{layout,action_policy:dashboard::ACTION_POLICY_SIGNAL.signal(),recording_status:recording,abort_only_mode:false,theme:dashboard::layout::ThemeConfig::default()}}
}
async fn native_terminal_dom(server:&Server)->(VirtualDom,dioxus::core::ElementId,dioxus::core::ElementId,tokio::sync::mpsc::UnboundedReceiver<QueuedCommand>) {
    let mut dom=VirtualDom::new(native_terminal_actions);setup(&dom,&server.base);policy(&dom,server,"terminal-1",true).await;
    let(tx,rx)=tokio::sync::mpsc::unbounded_channel();
    dom.in_scope(ScopeId::ROOT,||{
        *dashboard::WS_SENDER.write()=Some(dashboard::WsSender{tx});
    });
    let edits=dom.rebuild_to_vec();
    let id=|cmd:&str|edits.edits.iter().find_map(|edit|match edit {
        dioxus::core::Mutation::SetAttribute{name:"data-dispatch-command",value:dioxus::core::AttributeValue::Text(value),id,..}if value==cmd=>Some(*id),_=>None
    }).expect("actual ActionsTab command button");
    let(a,b)=(id("Underglow"),id("Abort"));
    dioxus::html::set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    (dom,a,b,rx)
}
fn native_terminal_event(dom:&VirtualDom,id:dioxus::core::ElementId,name:&'static str,value:Box<dyn std::any::Any>) {
    let data=dioxus::html::PlatformEventData::new(value);
    dom.runtime().handle_event(name,Event::new(Rc::new(data)as Rc<dyn std::any::Any>,true),id);
}
fn native_terminal_mouse(button:i16)->Box<dyn std::any::Any> {
    let mut value=serde_json::to_value(dioxus::html::SerializedMouseData::default()).unwrap();value["button"]=button.into();
    Box::new(serde_json::from_value::<dioxus::html::SerializedMouseData>(value).unwrap())
}
fn native_terminal_pointer(id:i32,button:i16,primary:bool,touch:bool)->Box<dyn std::any::Any> {
    let mut value=serde_json::to_value(dioxus::html::SerializedMouseData::default()).unwrap();
    let fields=serde_json::json!({"button":button,"pointer_id":id,"width":1.,"height":1.,"pressure":0.5,"tangential_pressure":0.,"tilt_x":0,"tilt_y":0,"twist":0,"pointer_type":if touch{"touch"}else{"mouse"},"is_primary":primary});
    value.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
    Box::new(serde_json::from_value::<dioxus::html::SerializedPointerData>(value).unwrap())
}
fn native_terminal_key(enter:bool,repeat:bool)->Box<dyn std::any::Any> {
    use dioxus::html::input_data::keyboard_types::{Code,Location,Modifiers};
    Box::new(dioxus::html::SerializedKeyboardData::new(if enter{Key::Enter}else{Key::Character(" ".into())},if enter{Code::Enter}else{Code::Space},Location::Standard,repeat,Modifiers::empty(),false))
}
fn native_terminal_touch(active:&[i32],changed:&[i32])->Box<dyn std::any::Any> {
    let points=|ids:&[i32]|ids.iter().map(|id|serde_json::json!({"identifier":id,"client_x":0.,"client_y":0.,"page_x":0.,"page_y":0.,"screen_x":0.,"screen_y":0.,"force":0.5,"radius_x":1.,"radius_y":1.,"rotation_angle":0.})).collect::<Vec<_>>();
    Box::new(serde_json::from_value::<dioxus::html::SerializedTouchData>(serde_json::json!({"alt_key":false,"ctrl_key":false,"meta_key":false,"shift_key":false,"touches":points(active),"changed_touches":points(changed),"target_touches":points(&active.iter().copied().filter(|id|changed.contains(id)).collect::<Vec<_>>())})).unwrap())
}
fn native_terminal_wire(rx:&mut tokio::sync::mpsc::UnboundedReceiver<QueuedCommand>,cmd:&str,epoch:&str) {
    let row=rx.try_recv().expect("explicit completed gesture must dispatch once");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&row.wire).unwrap(),serde_json::json!({"cmd":cmd,"policy_epoch":epoch}));assert!(rx.try_recv().is_err());
}
#[tokio::test(flavor="current_thread")]
async fn actual_native_pointer_click_waits_for_matching_current_pointer_up() {
    if !child(module_path!(),"actual_native_pointer_click_waits_for_matching_current_pointer_up"){return;}
    let server=Server::new();let(dom,a,_,mut rx)=native_terminal_dom(&server).await;
    native_terminal_event(&dom,a,"pointerdown",native_terminal_pointer(1,0,true,false));policy(&dom,&server,"terminal-2",true).await;
    native_terminal_event(&dom,a,"pointerdown",native_terminal_pointer(2,0,true,false));
    native_terminal_event(&dom,a,"click",native_terminal_mouse(0));
    assert!(rx.try_recv().is_err(),"NATIVE_OLD_CLICK_MUST_NOT_RELEASE_NEW_POINTER_BEFORE_UP");
    native_terminal_event(&dom,a,"pointerup",native_terminal_pointer(1,0,true,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));assert!(rx.try_recv().is_err());
    native_terminal_event(&dom,a,"pointerup",native_terminal_pointer(2,2,true,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));assert!(rx.try_recv().is_err());
    native_terminal_event(&dom,a,"pointerup",native_terminal_pointer(2,0,true,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));native_terminal_wire(&mut rx,"Underglow","terminal-2");
    policy(&dom,&server,"terminal-3",true).await;native_terminal_event(&dom,a,"pointerdown",native_terminal_pointer(3,0,true,false));native_terminal_event(&dom,a,"pointercancel",native_terminal_pointer(3,-1,true,false));native_terminal_event(&dom,a,"pointerup",native_terminal_pointer(3,0,true,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));assert!(rx.try_recv().is_err());
    native_terminal_event(&dom,a,"pointerdown",native_terminal_pointer(4,0,true,false));native_terminal_event(&dom,a,"pointerup",native_terminal_pointer(4,0,true,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));native_terminal_wire(&mut rx,"Underglow","terminal-3");
}
#[tokio::test(flavor="current_thread")]
async fn actual_native_mouse_and_right_pointer_use_actual_trigger_button() {
    if !child(module_path!(),"actual_native_mouse_and_right_pointer_use_actual_trigger_button"){return;}
    let server=Server::new();let(dom,a,_,mut rx)=native_terminal_dom(&server).await;
    native_terminal_event(&dom,a,"pointerdown",native_terminal_pointer(1,2,true,false));native_terminal_event(&dom,a,"pointerup",native_terminal_pointer(1,2,true,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(2));
    assert!(rx.try_recv().is_err(),"NATIVE_RIGHT_BUTTON_IS_NOT_ORDINARY_PRIMARY_INTENT");
    native_terminal_event(&dom,a,"mousedown",native_terminal_mouse(0));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));assert!(rx.try_recv().is_err());
    native_terminal_event(&dom,a,"mouseup",native_terminal_mouse(2));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));assert!(rx.try_recv().is_err());
    native_terminal_event(&dom,a,"mouseup",native_terminal_mouse(0));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));native_terminal_wire(&mut rx,"Underglow","terminal-1");
}
#[tokio::test(flavor="current_thread")]
async fn actual_native_touch_identifier_and_secondary_stop_terminal_are_retained() {
    if !child(module_path!(),"actual_native_touch_identifier_and_secondary_stop_terminal_are_retained"){return;}
    let server=Server::new();let(dom,a,b,mut rx)=native_terminal_dom(&server).await;
    native_terminal_event(&dom,a,"touchstart",native_terminal_touch(&[11],&[11]));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));
    assert!(rx.try_recv().is_err(),"NATIVE_TOUCH_REQUIRES_ORIGINAL_IDENTIFIER_TERMINAL");
    native_terminal_event(&dom,a,"touchend",native_terminal_touch(&[11],&[12]));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));assert!(rx.try_recv().is_err());
    native_terminal_event(&dom,a,"touchend",native_terminal_touch(&[],&[11]));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));native_terminal_wire(&mut rx,"Underglow","terminal-1");
    policy(&dom,&server,"terminal-2",true).await;native_terminal_event(&dom,a,"touchstart",native_terminal_touch(&[11],&[11]));
    native_terminal_event(&dom,b,"touchstart",native_terminal_touch(&[11,22],&[22]));native_terminal_event(&dom,a,"touchend",native_terminal_touch(&[22],&[11]));native_terminal_event(&dom,b,"click",native_terminal_mouse(0));assert!(rx.try_recv().is_err());
    // A new ordinary second finger cannot displace the explicit Stop owner.
    native_terminal_event(&dom,a,"touchstart",native_terminal_touch(&[11,22],&[11]));
    native_terminal_event(&dom,b,"touchend",native_terminal_touch(&[11],&[22]));native_terminal_event(&dom,b,"click",native_terminal_mouse(0));native_terminal_wire(&mut rx,"Abort","terminal-2");
}
#[tokio::test(flavor="current_thread")]
async fn actual_native_space_needs_key_up_but_enter_down_is_explicit() {
    if !child(module_path!(),"actual_native_space_needs_key_up_but_enter_down_is_explicit"){return;}
    let server=Server::new();let(dom,a,_,mut rx)=native_terminal_dom(&server).await;
    native_terminal_event(&dom,a,"keydown",native_terminal_key(false,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));
    assert!(rx.try_recv().is_err(),"NATIVE_SPACE_INTENT_REQUIRES_ITS_KEY_UP");
    native_terminal_event(&dom,a,"keyup",native_terminal_key(true,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));assert!(rx.try_recv().is_err());
    native_terminal_event(&dom,a,"keyup",native_terminal_key(false,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));native_terminal_wire(&mut rx,"Underglow","terminal-1");
    policy(&dom,&server,"terminal-2",true).await;native_terminal_event(&dom,a,"keydown",native_terminal_key(true,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));native_terminal_wire(&mut rx,"Underglow","terminal-2");
    policy(&dom,&server,"terminal-3",true).await;native_terminal_event(&dom,a,"keydown",native_terminal_key(false,true));native_terminal_event(&dom,a,"keyup",native_terminal_key(false,true));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));assert!(rx.try_recv().is_err());
    native_terminal_event(&dom,a,"keydown",native_terminal_key(false,false));native_terminal_event(&dom,a,"keyup",native_terminal_key(false,false));native_terminal_event(&dom,a,"click",native_terminal_mouse(0));native_terminal_wire(&mut rx,"Underglow","terminal-3");
}
#[tokio::test(flavor="current_thread")]
async fn actual_native_secondary_stop_cannot_be_consumed_by_unrelated_pointer_up() {
    if !child(module_path!(),"actual_native_secondary_stop_cannot_be_consumed_by_unrelated_pointer_up"){return;}
    let server=Server::new();let(dom,a,b,mut rx)=native_terminal_dom(&server).await;
    native_terminal_event(&dom,a,"pointerdown",native_terminal_pointer(11,0,true,true));policy(&dom,&server,"terminal-2",true).await;
    native_terminal_event(&dom,b,"pointerdown",native_terminal_pointer(22,0,false,true));native_terminal_event(&dom,a,"pointerup",native_terminal_pointer(11,0,true,true));native_terminal_event(&dom,b,"click",native_terminal_mouse(0));
    assert!(rx.try_recv().is_err(),"NATIVE_STOP_REQUIRES_ITS_OWN_POINTER_TERMINAL");
    native_terminal_event(&dom,b,"pointerup",native_terminal_pointer(22,0,false,true));native_terminal_event(&dom,b,"click",native_terminal_mouse(0));native_terminal_wire(&mut rx,"Abort","terminal-2");
}

fn exact_header<'a>(request:&'a str,name:&str)->Option<&'a str> {
    request.split("\r\n\r\n").next()?.lines().filter_map(|line|line.split_once(':'))
        .find(|(key,_)|key.eq_ignore_ascii_case(name)).map(|(_,value)|value.trim())
}
async fn origin_mode(dom:&VirtualDom,server:&Server,epoch:&str,form:Option<&str>) {
    let mut snapshot=dashboard::station_mode_contract::tests::example_snapshot();
    snapshot.policy_epoch=Some(epoch.into());
    let mut raw=serde_json::to_value(snapshot).unwrap();
    raw["control_origin_required"]=true.into();
    raw["control_origin"]=form.map_or(serde_json::Value::Null,|v|v.into());
    *server.mode.lock().unwrap()=raw;
    dashboard::incoming_epoch_native_tests::mode_get(dom).await;
}
fn origin_controls(dom:&VirtualDom,epoch:&str,origin:Option<&str>,closed:bool) {
    let mut data=fresh_policy();
    if closed { data["key_enabled"]=false.into();data["software_buttons_enabled"]=false.into(); }
    for control in data["controls"].as_array_mut().unwrap() {
        control["control_origin"]=origin.map_or(serde_json::Value::Null,|v|v.into());
    }
    dashboard::incoming_epoch_native_tests::receive_action_policy(dom,Some(epoch),data);
}
#[tokio::test(flavor="current_thread")]
async fn actual_action_origin_rotation_same_epoch_retires_queue_and_proxy_without_form_fallback() {
    if !child(module_path!(),"actual_action_origin_rotation_same_epoch_retires_queue_and_proxy_without_form_fallback"){return;}
    let server=Server::new();let dom=dom(&server.base);
    origin_mode(&dom,&server,"same-applied-epoch",None).await;
    origin_controls(&dom,"same-applied-epoch",Some("opaque-control-A"),false);
    let old=dom.in_scope(ScopeId::ROOT,||CommandOwner::capture("StartFill")).unwrap();
    let queued=dom.in_scope(ScopeId::ROOT,||QueuedCommand::capture(old.clone())).unwrap();
    let original_wire=queued.wire.clone();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&original_wire).unwrap()["control_origin"],"opaque-control-A");
    // A command's per-control origin is valid even with no generic form origin.
    assert!(dom.in_scope(ScopeId::ROOT,||dashboard::recording_download::ExportOwner::capture_ordinary_selected(||true)).is_err());
    let mut pending=Box::pin(post_primary(&old));
    origin_controls(&dom,"same-applied-epoch",Some("opaque-control-B"),false);
    assert!(!dom.in_scope(ScopeId::ROOT,||queued.owner.is_current()));
    assert_eq!(queued.wire,original_wire,"No replacement token may be written into an old queued packet");
    assert!(in_dom(&dom,pending.as_mut()).await.is_err());
    assert!(server.rows.lock().unwrap().is_empty());
    let fresh=dom.in_scope(ScopeId::ROOT,||CommandOwner::capture("StartFill")).unwrap();
    let mut send=Box::pin(post_primary(&fresh));in_dom(&dom,send.as_mut()).await.unwrap();
    let rows=server.rows.lock().unwrap();assert_eq!(rows.len(),1);
    assert!(rows[0].to_ascii_lowercase().contains("x-station-control-origin: opaque-control-b\r\n"));
    assert_eq!(exact_header(&rows[0],"X-Station-Control-Origin"),Some("opaque-control-B"));
    assert!(rows[0].to_ascii_lowercase().contains("x-station-policy-epoch: same-applied-epoch\r\n"));
    assert!(rows[0].ends_with("\"StartFill\""));drop(rows);
    origin_controls(&dom,"same-applied-epoch",None,false);
    assert!(dom.in_scope(ScopeId::ROOT,||CommandOwner::capture("StartFill")).is_err(),"Sticky required origin cannot fall back to an untagged command");
    assert_eq!(server.rows.lock().unwrap().len(),1);
}
#[tokio::test(flavor="current_thread")]
async fn actual_form_origin_rotation_before_first_poll_drops_old_body_and_sends_fresh_origin() {
    if !child(module_path!(),"actual_form_origin_rotation_before_first_poll_drops_old_body_and_sends_fresh_origin"){return;}
    let server=Server::new();let dom=dom(&server.base);
    origin_mode(&dom,&server,"same-form-epoch",Some("opaque-form-A")).await;
    let mut old=dom.in_scope(ScopeId::ROOT,||{
        let owner=dashboard::recording_download::ExportOwner::capture_ordinary_selected(||true).unwrap();
        Box::pin(dashboard::station_settings_request::post_captured::<_,serde_json::Value>("/api/gse/config",&serde_json::json!({"target":7}),Ok(owner)))
    });
    origin_mode(&dom,&server,"same-form-epoch",Some("opaque-form-B")).await;
    assert!(in_dom(&dom,old.as_mut()).await.is_err());assert!(server.rows.lock().unwrap().is_empty());
    let mut fresh=dom.in_scope(ScopeId::ROOT,||{
        let owner=dashboard::recording_download::ExportOwner::capture_ordinary_selected(||true).unwrap();
        Box::pin(dashboard::station_settings_request::post_captured::<_,serde_json::Value>("/api/gse/config",&serde_json::json!({"target":8}),Ok(owner)))
    });
    in_dom(&dom,fresh.as_mut()).await.unwrap();let rows=server.rows.lock().unwrap();assert_eq!(rows.len(),1);
    assert!(rows[0].to_ascii_lowercase().contains("x-station-control-origin: opaque-form-b\r\n"));
    assert_eq!(exact_header(&rows[0],"X-Station-Control-Origin"),Some("opaque-form-B"));
    assert!(rows[0].ends_with("{\"target\":8}"));
}
#[tokio::test(flavor="current_thread")]
async fn actual_emergency_origin_survives_ordinary_close_but_not_same_mode_recovery_rotation() {
    if !child(module_path!(),"actual_emergency_origin_survives_ordinary_close_but_not_same_mode_recovery_rotation"){return;}
    let server=Server::new();let dom=dom(&server.base);
    origin_mode(&dom,&server,"stop-applied-epoch",None).await;
    origin_controls(&dom,"stop-applied-epoch",Some("opaque-stop-before-recovery"),false);
    let old=dom.in_scope(ScopeId::ROOT,||CommandOwner::capture("Abort")).unwrap();
    // Actual owned ModeGET closes ordinary admissions at the SAME applied epoch.
    { let mut raw=server.mode.lock().unwrap();raw["admissions_open"]=false.into(); }
    dashboard::incoming_epoch_native_tests::mode_get(&dom).await;
    origin_controls(&dom,"stop-applied-epoch",Some("opaque-stop-before-recovery"),true);
    assert!(dom.in_scope(ScopeId::ROOT,||old.is_current()));
    assert!(dom.in_scope(ScopeId::ROOT,||dashboard::action_policy_control_enabled(&dashboard::ACTION_POLICY_SIGNAL.read(),"Abort")));
    assert!(dom.in_scope(ScopeId::ROOT,||CommandOwner::capture("StartFill")).is_err());
    let during_close=dom.in_scope(ScopeId::ROOT,||CommandOwner::capture("Abort")).unwrap();
    let wire=dom.in_scope(ScopeId::ROOT,||during_close.wire_message()).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&wire).unwrap()["control_origin"],"opaque-stop-before-recovery");
    origin_controls(&dom,"stop-applied-epoch",Some("opaque-stop-after-recovery"),true);
    assert!(!dom.in_scope(ScopeId::ROOT,||old.is_current()));assert!(!dom.in_scope(ScopeId::ROOT,||during_close.is_current()));
    let fresh=dom.in_scope(ScopeId::ROOT,||CommandOwner::capture("Abort")).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&dom.in_scope(ScopeId::ROOT,||fresh.wire_message()).unwrap()).unwrap()["control_origin"],"opaque-stop-after-recovery");
    assert!(server.rows.lock().unwrap().is_empty());
}

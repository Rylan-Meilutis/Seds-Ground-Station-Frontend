//! Actual mounted consumers, rendered event IDs and real held loopback HTTP.
use super::*;
use super::super as dashboard;
use dioxus::core::{AttributeValue, DynamicNode, ElementId, Task, TemplateNode, VNode};
use std::{cell::RefCell, collections::BTreeMap, io::{Read,Write}, net::{TcpListener,TcpStream}, sync::{Arc,Mutex,Condvar,atomic::{AtomicBool,Ordering}}, task::Poll, time::Duration};
type AdminRead = Rc<dyn Fn()->(bool,String)>;
thread_local! {
    static ADMIN_READ:RefCell<Option<AdminRead>>=const{RefCell::new(None)};
    static SPAWNED:RefCell<Vec<Task>>=const{RefCell::new(Vec::new())};
    static SHOW:RefCell<Option<Signal<bool>>>=const{RefCell::new(None)};
    static VIEW:RefCell<&'static str>=const{RefCell::new("admin")};
}
pub(crate) fn observe_admin_signals(read:impl Fn()->(bool,String)+'static){ADMIN_READ.with(|slot|*slot.borrow_mut()=Some(Rc::new(read)));}
fn admin_read()->AdminRead{ADMIN_READ.with(|slot|slot.borrow().as_ref().expect("actual mounted admin Signal observer").clone())}
pub(super) fn observe_actual_spawn(task:Task){SPAWNED.with(|v|v.borrow_mut().push(task));}
fn last_task()->Task{SPAWNED.with(|v|*v.borrow().last().expect("actual consumer task"))}
fn root()->Element{
    let show=use_signal(||true);SHOW.with(|v|*v.borrow_mut()=Some(show));
    if !*show.read(){return rsx!{p{"Unmounted"}};}
    if VIEW.with(|v|*v.borrow())=="replay"{
        rsx!{dashboard::recorded_replay_controls::RecordedReplayControls{broadcast:dashboard::live_stream_tab::BroadcastState::default()}}
    }else{rsx!{dashboard::admin_console::AdminConsole{on_exit:|_|{},on_navigate:|_|{}}}}
}
fn session(token:&str){
    crate::auth::set_current_session(crate::auth::StoredAuthSession{token:token.into(),remember_me:false,session:crate::auth::SessionStatus{
        authenticated:true,username:Some("neutral-operator".into()),roles:vec!["admin".into(),"stream_admin".into(),"stream_master".into()],
        permissions:crate::auth::Permissions{view_data:true,send_commands:true,..Default::default()},..Default::default()}});
}
fn dom(server:&Server,view:&'static str)->VirtualDom{
    VIEW.with(|v|*v.borrow_mut()=view);SPAWNED.with(|v|v.borrow_mut().clear());
    let mut dom=VirtualDom::new(root);dom.in_scope(ScopeId::ROOT,||{
        dashboard::UrlConfig::set_base_url_and_persist(server.base.clone());
        *dashboard::WS_EPOCH.write()=7;
        *dashboard::DASHBOARD_LIFE.write()=dashboard::DashboardLife::new_alive();
        *dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap()=dashboard::FrontendNetworkMetrics{ws_connected:true,..Default::default()};
        session("neutral-original");
    });dom.rebuild_in_place();dom
}
fn isolated(name:&str)->bool{dashboard::command_owner::isolated_test_child(module_path!(),name)}
fn html(dom:&VirtualDom)->String{dioxus::ssr::render(dom)}
fn render(dom:&mut VirtualDom){dom.render_immediate_to_vec();dom.process_events();dom.render_immediate_to_vec();}
// This narrow fixture mounts the real Admin component, not the whole Dashboard
// which owns the existing presentation clock driver. Deliver its exact logical
// tick event; this changes no time, auth revision, policy epoch or owner.
fn presentation_tick(dom:&VirtualDom){
    dom.in_scope(ScopeId::ROOT,||{
        let mut tick=dashboard::HEADER_CLOCK_TICK.write();
        *tick=tick.wrapping_add(1);
    });
}
async fn until(dom:&mut VirtualDom,server:&Server,condition:impl Fn(&VirtualDom)->bool){
    tokio::time::timeout(Duration::from_secs(3),async{loop{
        let changed=server.changed.notified();render(dom);if condition(dom){return;}
        tokio::select!{_=dom.wait_for_work()=>{},_=changed=>{}}
    }}).await.expect("actual component/HTTP barrier");
}
#[derive(Clone)]
struct Request{method:String,path:String,headers:BTreeMap<String,String>,body:serde_json::Value}
struct Held{rule:Option<(String,String)>,released:bool}
struct Server{
    base:String,rows:Arc<Mutex<Vec<Request>>>,completed:Arc<Mutex<Vec<(String,String)>>>,changed:Arc<tokio::sync::Notify>,hold:Arc<(Mutex<Held>,Condvar)>,
    stop:Arc<AtomicBool>,status:Arc<std::sync::atomic::AtomicU16>,user:Arc<Mutex<String>>,
    mode:Arc<Mutex<serde_json::Value>>,worker:Option<std::thread::JoinHandle<()>>,
}
impl Server{
    fn new()->Self{
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();let base=format!("http://{}",listener.local_addr().unwrap());
        let rows=Arc::new(Mutex::new(Vec::new()));let completed=Arc::new(Mutex::new(Vec::new()));let changed=Arc::new(tokio::sync::Notify::new());
        let hold=Arc::new((Mutex::new(Held{rule:None,released:true}),Condvar::new()));let stop=Arc::new(AtomicBool::new(false));
        let status=Arc::new(std::sync::atomic::AtomicU16::new(200));let user=Arc::new(Mutex::new("target-original".to_owned()));
        let mode=Arc::new(Mutex::new(serde_json::to_value(dashboard::station_mode_contract::tests::example_snapshot()).unwrap()));
        let(rows_,completed_,changed_,hold_,stop_,status_,user_,mode_)=(rows.clone(),completed.clone(),changed.clone(),hold.clone(),stop.clone(),status.clone(),user.clone(),mode.clone());
        let worker=std::thread::spawn(move||{
            let mut children=Vec::new();
            while !stop_.load(Ordering::Acquire){
                let(mut socket,_)=listener.accept().unwrap();if stop_.load(Ordering::Acquire){break;}
                let(rows,completed,changed,hold,stop,status,user,mode)=(rows_.clone(),completed_.clone(),changed_.clone(),hold_.clone(),stop_.clone(),status_.clone(),user_.clone(),mode_.clone());
                children.push(std::thread::spawn(move||{
                    socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();let mut raw=Vec::new();let mut buf=[0;4096];
                    let split=loop{let n=socket.read(&mut buf).unwrap();assert!(n>0);raw.extend_from_slice(&buf[..n]);assert!(raw.len()<65536);if let Some(i)=raw.windows(4).position(|v|v==b"\r\n\r\n"){break i+4;}};
                    let text=String::from_utf8(raw[..split].to_vec()).unwrap();let mut words=text.lines().next().unwrap().split_whitespace();
                    let method=words.next().unwrap().to_owned();let path=words.next().unwrap().to_owned();
                    let headers:BTreeMap<String,String>=text.lines().skip(1).filter_map(|v|v.split_once(':').map(|(k,v)|(k.to_ascii_lowercase(),v.trim().into()))).collect();
                    let length=headers.get("content-length").map(|v|v.parse::<usize>().unwrap()).unwrap_or(0);
                    while raw.len()<split+length{let n=socket.read(&mut buf).unwrap();assert!(n>0);raw.extend_from_slice(&buf[..n]);assert!(raw.len()<65536);}
                    let body=if length==0{serde_json::Value::Null}else{serde_json::from_slice(&raw[split..split+length]).unwrap()};
                    let held=hold.0.lock().unwrap().rule.as_ref()==Some(&(method.clone(),path.clone()));
                    let code=if held{status.load(Ordering::Acquire)}else{200};
                    let account=serde_json::json!({"username":*user.lock().unwrap(),"roles":[],"permissions":{"view_data":true,"send_commands":false,"set_system_time":false,"voice_transmit":false,"camera_publish":false,"sign_checklists":false},"command_access":{"allowed_commands":[]},"calibration_access":{"view":false,"edit":false},"disabled":false});
                    // Capture reply BEFORE hold: changing actor/config cannot rewrite old response provenance.
                    let reply=match(method.as_str(),path.as_str()){
                        ("GET","/api/admin/users")=>serde_json::json!([account]),
                        ("POST","/api/admin/users")|("POST","/api/admin/users/create")=>{let mut v=body.clone();v.as_object_mut().unwrap().remove("password");v},
                        ("POST","/api/admin/users/delete")=>serde_json::json!({"deleted":body["username"]}),
                        ("GET","/api/station_positions")=>serde_json::json!({"revision":0,"ground_station":null,"rrls":null}),
                        ("GET","/api/station/pairing")=>serde_json::json!({"role":"primary","paired":false,"source":null,"error":null,"peers":[],"fingerprint":null,"restart_required":false,"can_manage":true,"radio_available":true,"unpair_available":false,"exchange":null,"primary_link":null}),
                        ("GET","/api/recordings")=>serde_json::json!([{"id":"record-r","bytes":1,"active":false}]),
                        ("GET","/api/live_streams")=>serde_json::to_value(dashboard::live_stream_tab::LiveStreamConfig::default()).unwrap(),
                        ("GET","/api/recordings/record-r/replay/flight")=>serde_json::json!({"upper_id":42,"start_ms":100,"end_ms":1000}),
                        ("POST","/api/media-assets/recorded-vehicle/control")=>body.clone(),
                        ("GET","/api/station-modes")=>mode.lock().unwrap().clone(),
                        _=>panic!("unexpected local fixture route {method} {path}"),
                    };
                    let completion=(method.clone(),path.clone());
                    {let mut v=rows.lock().unwrap();assert!(v.len()<128);v.push(Request{method,path,headers,body});}changed.notify_waiters();
                    if held{let mut gate=hold.0.lock().unwrap();while !gate.released&&!stop.load(Ordering::Acquire){gate=hold.1.wait(gate).unwrap();}}
                    let bytes=serde_json::to_vec(&reply).unwrap();
                    let _=write!(socket,"HTTP/1.1 {code} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len());let _=socket.write_all(&bytes);completed.lock().unwrap().push(completion);changed.notify_waiters();
                }));
            }for child in children{child.join().unwrap();}
        });Self{base,rows,completed,changed,hold,stop,status,user,mode,worker:Some(worker)}
    }
    fn hold(&self,method:&str,path:&str,status:u16){let mut h=self.hold.0.lock().unwrap();h.rule=Some((method.into(),path.into()));h.released=false;self.status.store(status,Ordering::Release);}
    fn release(&self){self.hold.0.lock().unwrap().released=true;self.hold.1.notify_all();self.changed.notify_waiters();}
    fn replies(&self,method:&str,path:&str)->usize{self.completed.lock().unwrap().iter().filter(|(m,p)|m==method&&p==path).count()}
    fn count(&self,method:&str,path:&str)->usize{self.rows.lock().unwrap().iter().filter(|r|r.method==method&&r.path==path).count()}
}
impl Drop for Server{fn drop(&mut self){self.stop.store(true,Ordering::Release);self.release();let _=TcpStream::connect(self.base.trim_start_matches("http://"));self.worker.take().unwrap().join().unwrap();}}
fn nodes(dom:&VirtualDom,node:&VNode,out:&mut Vec<VNode>){
    out.push(node.clone());for(index,dynamic)in node.dynamic_nodes.iter().enumerate(){match dynamic{
        DynamicNode::Fragment(v)=>for n in v{nodes(dom,n,out)},
        DynamicNode::Component(c)=>if let Some(s)=c.mounted_scope(index,node,dom){nodes(dom,s.root_node(),out)},_=>{},
    }}
}
fn text(dom:&VirtualDom,node:&VNode,t:&TemplateNode)->String{match t{
    TemplateNode::Text{text}=>text.to_string(),
    TemplateNode::Element{children,..}=>children.iter().map(|t|text(dom,node,t)).collect(),
    TemplateNode::Dynamic{id}=>match &node.dynamic_nodes[*id]{
        DynamicNode::Text(v)=>v.value.clone(),
        DynamicNode::Fragment(v)=>v.iter().map(|n|n.template.roots.iter().map(|t|text(dom,n,t)).collect::<String>()).collect(),
        DynamicNode::Component(c)=>c.mounted_scope(*id,node,dom).map(|s|s.root_node().template.roots.iter().map(|t|text(dom,s.root_node(),t)).collect()).unwrap_or_default(),
        DynamicNode::Placeholder(_)=>String::new(),
    },
}}
fn at<'a>(node:&'a VNode,path:&[u8])->&'a TemplateNode{
    let mut t=&node.template.roots[usize::from(path[0])];
    for p in &path[1..]{let TemplateNode::Element{children,..}=t else{panic!("actual template path")};t=&children[usize::from(*p)];}t
}
fn element(dom:&VirtualDom,tag:&str,label:&str,event:&str)->ElementId{
    let mut all=Vec::new();nodes(dom,dom.base_scope().root_node(),&mut all);let mut found=Vec::new();
    for node in &all{for(index,attrs)in node.dynamic_attrs.iter().enumerate(){
        if !attrs.iter().any(|a|(a.name==event||a.name==format!("on{event}"))&&matches!(a.value,AttributeValue::Listener(_))){continue;}
        let path=node.template.attr_paths[index];let target=at(node,path);let TemplateNode::Element{tag:actual,..}=target else{continue;};if *actual!=tag{continue;}
        let labelnode=if tag=="input"||tag=="select"{at(node,&path[..path.len()-1])}else{target};
        if text(dom,node,labelnode).contains(label){found.push(node.mounted_dynamic_attribute(index,dom).expect("actual mounted event id"));}
    }}assert_eq!(found.len(),1,"unique actual {tag}/{label}/{event}");found[0]
}
fn click(dom:&VirtualDom,label:&str){
    dioxus::html::set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    let payload=dioxus::html::PlatformEventData::new(Box::new(dioxus::html::SerializedMouseData::default()));
    dom.runtime().handle_event("click",Event::new(Rc::new(payload)as Rc<dyn std::any::Any>,true),element(dom,"button",label,"click"));
}
fn form(dom:&VirtualDom,tag:&str,label:&str,event:&str,value:&str){
    dioxus::html::set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    let payload=dioxus::html::PlatformEventData::new(Box::new(dioxus::html::SerializedFormData::new(value.into(),Vec::new())));
    dom.runtime().handle_event(event,Event::new(Rc::new(payload)as Rc<dyn std::any::Any>,true),element(dom,tag,label,event));
}

fn assert_original_post(server:&Server,path:&str){
    let rows=server.rows.lock().unwrap();let posts:Vec<_>=rows.iter().filter(|r|r.method=="POST"&&r.path==path).collect();
    assert_eq!(posts.len(),1);let row=posts[0];
    assert_eq!(row.headers.get("authorization").map(String::as_str),Some("Bearer neutral-original"));
    assert!(!row.headers.contains_key("x-station-policy-epoch"));
    assert!(!row.headers.contains_key("x-station-control-origin"));
    assert_eq!(row.body,serde_json::json!({"username":"target-original","roles":[],"permissions":{"view_data":true,"send_commands":false,"set_system_time":false,"voice_transmit":false,"camera_publish":false,"sign_checklists":false},"command_access":{"allowed_commands":[]},"calibration_access":{"view":false,"edit":false},"disabled":false}));
}
#[tokio::test(flavor="current_thread")]
async fn actual_admin_positive_and_closed_mode_neutral_action_keep_body_owner_and_rtt(){
    if !isolated("actual_admin_positive_and_closed_mode_neutral_action_keep_body_owner_and_rtt"){return;}
    for closed in [false,true]{
        let server=Server::new();let mut dom=dom(&server,"admin");
        until(&mut dom,&server,|d|html(d).contains("Save target-original")).await;
        assert!(dashboard::FRONTEND_NETWORK_METRICS_STATE.lock().unwrap().http_rtt_ms.is_some());
        if closed{
            server.mode.lock().unwrap()["admissions_open"]=false.into();
            dashboard::incoming_epoch_native_tests::mode_get(&dom).await;
            assert!(dom.in_scope(ScopeId::ROOT,dashboard::station_policy_context::capture_current).is_err());
        }
        click(&dom,"Save target-original");let task=last_task();
        until(&mut dom,&server,|d|html(d).contains("Account saved")).await;
        assert!(dom.in_scope(ScopeId::ROOT,||task.poll_now()).is_ready());
        assert_original_post(&server,"/api/admin/users");
        assert_eq!(admin_read()(),(false,"Account saved".into()),"actual successful component cleanup Signals");
        assert!(!html(&dom).contains("This panel has retired"));
    }
}
#[tokio::test(flavor="current_thread")]
async fn actual_admin_before_firstpoll_auth_and_base_aba_are_zero_io(){
    if !isolated("actual_admin_before_firstpoll_auth_and_base_aba_are_zero_io"){return;}
    for change in ["auth","base","auth-aba","base-aba"]{
        let server=Server::new();let replacement=Server::new();let mut dom=dom(&server,"admin");
        until(&mut dom,&server,|d|html(d).contains("Save target-original")).await;
        click(&dom,"Save target-original");let task=last_task();
        // No task poll/await between actual listener and revision replacement.
        dom.in_scope(ScopeId::ROOT,||match change{
            "auth"=>session("neutral-replacement"),
            "base"=>dashboard::UrlConfig::set_base_url_and_persist(replacement.base.clone()),
            "auth-aba"=>{session("neutral-replacement");session("neutral-original");},
            "base-aba"=>{dashboard::UrlConfig::set_base_url_and_persist(replacement.base.clone());dashboard::UrlConfig::set_base_url_and_persist(server.base.clone());},
            _=>unreachable!(),
        });
        assert!(matches!(dom.in_scope(ScopeId::ROOT,||task.poll_now()),Poll::Ready(())),"actual spawned operation must terminate before creating HTTP");
        render(&mut dom);
        assert_eq!(server.count("POST","/api/admin/users"),0);
        assert_eq!(replacement.count("POST","/api/admin/users"),0);
        assert!(html(&dom).contains("This panel has retired"));
        assert!(!html(&dom).contains("Account saved"));
    }
}
#[tokio::test(flavor="current_thread")]
async fn actual_replay_get_to_post_retirement_does_not_borrow_replacement_auth(){
    if !isolated("actual_replay_get_to_post_retirement_does_not_borrow_replacement_auth"){return;}
    let server=Server::new();let mut dom=dom(&server,"replay");
    until(&mut dom,&server,|d|html(d).contains("record-r")).await;
    form(&dom,"select","Recording","change","record-r");render(&mut dom);
    server.hold("GET","/api/recordings/record-r/replay/flight",200);
    click(&dom,"Show 3D replay");let task=last_task();
    assert!(dom.in_scope(ScopeId::ROOT,||task.poll_now()).is_pending());
    until(&mut dom,&server,|_|server.count("GET","/api/recordings/record-r/replay/flight")==1).await;
    dom.in_scope(ScopeId::ROOT,||{session("neutral-replacement");session("neutral-original");});
    server.release();
    until(&mut dom,&server,|_|server.replies("GET","/api/recordings/record-r/replay/flight")==1).await;
    assert!(dom.in_scope(ScopeId::ROOT,||task.poll_now()).is_ready());
    presentation_tick(&dom);
    render(&mut dom);
    assert_eq!(server.count("POST","/api/media-assets/recorded-vehicle/control"),0);
    assert!(html(&dom).contains("This panel has retired"));
    assert!(!html(&dom).contains("Recorded flight replay saved"));
}
#[tokio::test(flavor="current_thread")]
async fn actual_held401_and_cleanup_cannot_paint_replacement_mount_or_clear_new_auth(){
    if !isolated("actual_held401_and_cleanup_cannot_paint_replacement_mount_or_clear_new_auth"){return;}
    let server=Server::new();let mut dom=dom(&server,"admin");
    until(&mut dom,&server,|d|html(d).contains("Save target-original")).await;
    server.hold("POST","/api/admin/users",401);
    click(&dom,"Save target-original");let task=last_task();
    assert!(dom.in_scope(ScopeId::ROOT,||task.poll_now()).is_pending());
    until(&mut dom,&server,|_|server.count("POST","/api/admin/users")==1).await;
    let read_original=admin_read();
    let held_signals=read_original();
    assert_eq!(held_signals,(true,String::new()),"real original mounted request owns busy before retirement");
    dom.in_scope(ScopeId::ROOT,||session("neutral-replacement"));
    presentation_tick(&dom);
    render(&mut dom);assert!(html(&dom).contains("This panel has retired"));
    server.release();
    until(&mut dom,&server,|_|server.replies("POST","/api/admin/users")==1).await;
    assert!(dom.in_scope(ScopeId::ROOT,||task.poll_now()).is_ready());
    render(&mut dom);
    assert_eq!(dom.in_scope(ScopeId::ROOT,crate::auth::current_token).as_deref(),Some("neutral-replacement"));
    assert!(!html(&dom).contains("HTTP 401"));assert!(!html(&dom).contains("Account saved"));
    assert_eq!(read_original(),held_signals,"late old response/error/cleanup must not change ORIGINAL still-mounted Signals, independent of hidden DOM");
    assert_original_post(&server,"/api/admin/users");
    // Only explicit actual unmount/reopen may load replacement data/draft.
    dom.in_scope(ScopeId::ROOT,||SHOW.with(|v|v.borrow().unwrap().set(false)));render(&mut dom);
    assert!(html(&dom).contains("Unmounted"));
    *server.user.lock().unwrap()="target-replacement".into();
    dom.in_scope(ScopeId::ROOT,||SHOW.with(|v|v.borrow().unwrap().set(true)));render(&mut dom);
    until(&mut dom,&server,|d|html(d).contains("Save target-replacement")).await;
    assert!(!html(&dom).contains("This panel has retired"));assert!(!html(&dom).contains("Account saved"));
}
#[tokio::test(flavor="current_thread")]
async fn actual_unmount_reopen_clears_secret_draft_and_old_response_cannot_restore_it(){
    if !isolated("actual_unmount_reopen_clears_secret_draft_and_old_response_cannot_restore_it"){return;}
    let server=Server::new();let mut dom=dom(&server,"admin");
    until(&mut dom,&server,|d|html(d).contains("Save target-original")).await;
    form(&dom,"input","Username","input","draft-user");render(&mut dom);
    form(&dom,"input","Temporary password","input","DummyOnlySecret0123");render(&mut dom);
    assert!(html(&dom).contains("draft-user"));assert!(html(&dom).contains("DummyOnlySecret0123"));
    server.hold("POST","/api/admin/users",200);click(&dom,"Save target-original");let task=last_task();
    assert!(dom.in_scope(ScopeId::ROOT,||task.poll_now()).is_pending());
    until(&mut dom,&server,|_|server.count("POST","/api/admin/users")==1).await;
    dom.in_scope(ScopeId::ROOT,||SHOW.with(|v|v.borrow().unwrap().set(false)));render(&mut dom);
    dom.in_scope(ScopeId::ROOT,||session("neutral-replacement"));
    *server.user.lock().unwrap()="target-replacement".into();
    dom.in_scope(ScopeId::ROOT,||SHOW.with(|v|v.borrow().unwrap().set(true)));render(&mut dom);
    until(&mut dom,&server,|d|html(d).contains("Save target-replacement")).await;
    server.release();until(&mut dom,&server,|_|server.replies("POST","/api/admin/users")==1).await;render(&mut dom);
    let output=html(&dom);assert!(!output.contains("draft-user"));assert!(!output.contains("DummyOnlySecret0123"));
    assert!(!output.contains("Account saved"));assert!(output.contains("Save target-replacement"));
    assert_eq!(server.count("POST","/api/admin/users"),1);
}

#[tokio::test(flavor="current_thread")]
async fn actual_current_owner401_is_original_error_without_auth_clear_or_retry(){
    if !isolated("actual_current_owner401_is_original_error_without_auth_clear_or_retry"){return;}
    let server=Server::new();let mut dom=dom(&server,"admin");
    until(&mut dom,&server,|d|html(d).contains("Save target-original")).await;
    server.hold("POST","/api/admin/users",401);click(&dom,"Save target-original");let task=last_task();
    assert!(dom.in_scope(ScopeId::ROOT,||task.poll_now()).is_pending());
    until(&mut dom,&server,|_|server.count("POST","/api/admin/users")==1).await;
    server.release();until(&mut dom,&server,|d|html(d).contains("401")).await;
    assert!(dom.in_scope(ScopeId::ROOT,||task.poll_now()).is_ready());
    assert_eq!(dom.in_scope(ScopeId::ROOT,crate::auth::current_token).as_deref(),Some("neutral-original"));
    assert!(!html(&dom).contains("This panel has retired"));assert!(!html(&dom).contains("Account saved"));
    assert_original_post(&server,"/api/admin/users");
    let (busy,message)=admin_read()();assert!(!busy);assert!(message.contains("401"),"unchanged-owner actual error/cleanup remains visible to original Signal observer");
}

#[tokio::test(flavor="current_thread")]
async fn actual_admin_delete_requires_confirmation_then_removes_the_row(){
    if !isolated("actual_admin_delete_requires_confirmation_then_removes_the_row"){return;}
    let server=Server::new();let mut dom=dom(&server,"admin");
    until(&mut dom,&server,|d|html(d).contains("Save target-original")).await;
    assert!(html(&dom).contains("1 accounts · 1 active"));
    click(&dom,"Delete target-original");render(&mut dom);
    assert!(html(&dom).contains("Delete target-original?"),"destructive action asks first");
    assert_eq!(server.count("POST","/api/admin/users/delete"),0);
    click(&dom,"Delete permanently");
    until(&mut dom,&server,|d|html(d).contains("Deleted target-original")).await;
    assert_eq!(server.count("POST","/api/admin/users/delete"),1);
    let rows=server.rows.lock().unwrap();
    let row=rows.iter().find(|r|r.path=="/api/admin/users/delete").unwrap();
    assert_eq!(row.body,serde_json::json!({"username":"target-original"}));
    assert_eq!(row.headers.get("authorization").map(String::as_str),Some("Bearer neutral-original"));
    drop(rows);
    assert!(!html(&dom).contains("Save target-original"));
}

use super::*;
use std::{future::Future, io::{Read, Write}, net::TcpListener, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use super::super::super::incoming_epoch_native_tests as incoming;

thread_local! {
    static MOUNT_DOM: std::cell::RefCell<Option<VirtualDom>> = const { std::cell::RefCell::new(None) };
    static MOUNT_ALIVE: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) };
}
fn mount_component() -> Element {
    let alive=super::super::super::recording_download::use_export_lifetime();
    MOUNT_ALIVE.with(|slot|*slot.borrow_mut()=Some(alive));
    rsx! { div {} }
}
fn mounted_alive() -> Arc<AtomicBool> {
    let mut dom=VirtualDom::new(mount_component);dom.rebuild_in_place();
    MOUNT_DOM.with(|slot|*slot.borrow_mut()=Some(dom));
    MOUNT_ALIVE.with(|slot|slot.borrow().as_ref().unwrap().clone())
}
type Rows = Arc<Mutex<Vec<String>>>;
struct Server {
    base: String,
    mode: Arc<Mutex<serde_json::Value>>,
    unavailable: Arc<AtomicBool>,
    hold: Arc<AtomicBool>,
    rows: Rows,
    end: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();listener.set_nonblocking(true).unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());
        let mode=Arc::new(Mutex::new(serde_json::to_value(super::super::super::station_mode_contract::tests::example_snapshot()).unwrap()));
        let unavailable=Arc::new(AtomicBool::new(false));let hold=Arc::new(AtomicBool::new(false));
        let rows:Rows=Arc::new(Mutex::new(Vec::new()));let end=Arc::new(AtomicBool::new(false));
        let (m,u,h,r,e)=(mode.clone(),unavailable.clone(),hold.clone(),rows.clone(),end.clone());
        let thread=std::thread::spawn(move||{
            let mut workers=Vec::new();
            while !e.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut socket,_))=>{
                        let (m,u,h,r,e)=(m.clone(),u.clone(),h.clone(),r.clone(),e.clone());
                        workers.push(std::thread::spawn(move||{
                            socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                            let mut bytes=Vec::new();let mut buffer=[0;4096];
                            loop {
                                let n=socket.read(&mut buffer).unwrap();if n==0 {break;}bytes.extend_from_slice(&buffer[..n]);assert!(bytes.len()<65536);
                                if let Some(split)=bytes.windows(4).position(|v|v==b"\r\n\r\n") {
                                    let headers=String::from_utf8_lossy(&bytes[..split]).to_ascii_lowercase();
                                    let size=headers.lines().find_map(|line|line.strip_prefix("content-length: ")).map(|v|v.parse::<usize>().unwrap()).unwrap_or(0);
                                    if bytes.len()>=split+4+size {break;}
                                }
                            }
                            let request=String::from_utf8(bytes).unwrap();let path=request.lines().next().unwrap().split_whitespace().nth(1).unwrap();
                            r.lock().unwrap().push(request.clone());
                            let mode=m.lock().unwrap().clone();
                            let is_mode=path=="/api/station-modes";
                            let is_status=path==STATUS;
                            assert!(is_mode||is_status||path=="/api/board-recovery/prepare","unexpected path {path}");
                            let status:serde_json::Value=serde_json::from_str(include_str!("../../tests/fixtures/board-recovery/status-example.json")).unwrap();
                            let body=if is_mode {mode.clone()} else if is_status {status.clone()} else {
                                serde_json::json!({"ticket":"fresh-refresh-ticket","backend_instance":status["backend_instance"],"principal_context":status["principal"]["context"],"reset_epoch":status["reset_epoch"],"expires_in_ms":30000,"action":"reset_startup","targets":[],"summary":"New review, not a confirm","confirmation":"RESET ALL STARTUP; VERIFY NONE"})
                            };
                            if is_status {let started=Instant::now();while h.load(Ordering::Acquire)&&!e.load(Ordering::Acquire){assert!(started.elapsed()<Duration::from_secs(5));std::thread::sleep(Duration::from_millis(1));}}
                            let body=serde_json::to_vec(&body).unwrap();let code=if is_mode&&u.load(Ordering::Acquire){503}else{200};
                            let metadata=if is_mode {String::new()} else {format!("X-Station-Epoch-Scope: mode\r\nX-Station-Policy-Epoch: {}\r\n",mode["policy_epoch"].as_str().unwrap())};
                            let _=write!(socket,"HTTP/1.1 {code} Fixture\r\nContent-Type: application/json\r\n{metadata}Content-Length: {}\r\nConnection: close\r\n\r\n",body.len());let _=socket.write_all(&body);
                        }));
                    }
                    Err(error) if error.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(1)),
                    Err(error)=>panic!("{error}"),
                }
            }
            for worker in workers {worker.join().unwrap();}
        });
        Self {base,mode,unavailable,hold,rows,end,thread:Some(thread)}
    }
    fn posts(&self)->usize {self.rows.lock().unwrap().iter().filter(|row|row.starts_with("POST ")).count()}
    fn status_reads(&self)->usize {self.rows.lock().unwrap().iter().filter(|row|row.starts_with("GET /api/board-recovery/status ")).count()}
}
impl Drop for Server {fn drop(&mut self){self.end.store(true,Ordering::Release);self.hold.store(false,Ordering::Release);self.thread.take().unwrap().join().unwrap();}}
async fn in_dom<F:std::future::Future+?Sized>(dom:&VirtualDom,mut future:std::pin::Pin<&mut F>)->F::Output {
    std::future::poll_fn(|cx|dom.in_scope(ScopeId::ROOT,||future.as_mut().poll(cx))).await
}
fn child(name:&str,cases:&[&str])->Option<String>{
    if let Ok(case)=std::env::var("GS_REFRESH_CASE"){return Some(case);}
    for case in cases {
        let root=std::env::temp_dir().join(format!("gs-refresh-{}-{case}",std::process::id()));std::fs::create_dir(&root).unwrap();
        let result=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact",&format!("telemetry_dashboard::board_recovery_panel::tests::refresh_native_tests::{name}"),"--nocapture"])
            .env("GS_REFRESH_CASE",case).env("XDG_DATA_HOME",root.join("data")).env("XDG_CONFIG_HOME",root.join("config")).current_dir(&root).output().unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(result.status.success(),"{case}: {}\n{}",String::from_utf8_lossy(&result.stdout),String::from_utf8_lossy(&result.stderr));
        assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    }None
}
async fn review(dom:&VirtualDom)->Pending {
    let mut request=dom.in_scope(ScopeId::ROOT,||Box::pin(read_status()));let snapshot=in_dom(dom,request.as_mut()).await.unwrap();
    dom.in_scope(ScopeId::ROOT,||Pending {
        owner:snapshot.policy_owner.clone().unwrap(),challenge:serde_json::from_value(serde_json::json!({"ticket":"original-ticket","backend_instance":snapshot.status.backend_instance,"principal_context":snapshot.status.principal.context,"reset_epoch":snapshot.status.reset_epoch,"expires_in_ms":30000,"action":"reset_startup","targets":[],"summary":"Original review","confirmation":"RESET ALL STARTUP; VERIFY NONE"})).unwrap(),
        snapshot,received:super::super::super::monotonic_now_ms(),action:"reset_startup".into(),board:None,mode:None,
    })
}
async fn change(case:&str,dom:&VirtualDom,server:&Server,alive:&Arc<AtomicBool>){
    match case {
        "mode"=>{server.mode.lock().unwrap()["policy_epoch"]="owner-A:epoch-2".into();incoming::mode_get(dom).await;},
        "auth"=>dom.in_scope(ScopeId::ROOT,||{let mut session=crate::auth::current_status();session.authenticated=true;crate::auth::set_current_session(crate::auth::StoredAuthSession{token:"replacement-token".into(),remember_me:false,session});}),
        "base"=>dom.in_scope(ScopeId::ROOT,||{*super::super::super::BASE_URL.write()="http://127.0.0.1:1".into();}),
        "mount"=>{MOUNT_DOM.with(|slot|drop(slot.borrow_mut().take()));assert!(!alive.load(Ordering::Acquire),"actual VDom drop must retire the production mount lifetime");},
        _=>panic!("unknown change"),
    }
}
#[tokio::test(flavor="current_thread")]
async fn actual_refresh_after_same_applied_outage_captures_new_click_owner_without_reviving_confirm(){
    if child("actual_refresh_after_same_applied_outage_captures_new_click_owner_without_reviving_confirm",&["positive"]).is_none(){return;}
    let server=Server::new();let dom=incoming::dom(&server.base);incoming::mode_get(&dom).await;
    let original=review(&dom).await;
    server.unavailable.store(true,Ordering::Release);assert!(incoming::mode_get_completion(&dom).await.is_err());
    server.unavailable.store(false,Ordering::Release);incoming::mode_get(&dom).await;
    dom.in_scope(ScopeId::ROOT,||{assert!(!original.owner.is_current());assert!(original.snapshot.review_current());});
    let intent=dom.in_scope(ScopeId::ROOT,||RefreshIntent::capture(&original,Arc::new(AtomicBool::new(true)))).unwrap();
    let mut request=Box::pin(intent.read());let fresh=in_dom(&dom,request.as_mut()).await.unwrap();
    let mut prepare=dom.in_scope(ScopeId::ROOT,||Box::pin(prepare_request(&fresh,intent.owner.clone(),&intent.original.action,None,None)));
    let challenge=in_dom(&dom,prepare.as_mut()).await.unwrap();
    assert_eq!(challenge.ticket,"fresh-refresh-ticket");assert_eq!(server.posts(),1);
    dom.in_scope(ScopeId::ROOT,||{assert!(!original.owner.is_current());assert!(!original.valid(&fresh));assert!(intent.owner.is_current());assert!(std::rc::Rc::ptr_eq(fresh.policy_owner.as_ref().unwrap(),&intent.owner));});
    let rows=server.rows.lock().unwrap();let post=rows.iter().find(|row|row.starts_with("POST ")).unwrap();
    assert!(post.to_ascii_lowercase().contains("x-station-policy-epoch: owner-a:epoch-1"));
    assert!(post.contains("\"action\":\"reset_startup\""));assert!(!post.contains("original-ticket"));
    assert!(rows.iter().all(|row|!row.starts_with("POST /api/board-recovery/confirm ")));
}
#[tokio::test(flavor="current_thread")]
async fn actual_refresh_rejects_changed_original_review_before_click_or_first_poll(){
    let Some(case)=child("actual_refresh_rejects_changed_original_review_before_click_or_first_poll",&["click-mode","click-auth","click-base","click-mount","poll-mode","poll-auth","poll-base","poll-mount","prepare-mode","prepare-auth","prepare-base","prepare-mount"])else{return;};
    let server=Server::new();let dom=incoming::dom(&server.base);incoming::mode_get(&dom).await;let original=review(&dom).await;
    let alive=mounted_alive();let before=server.status_reads();
    let (phase,kind)=case.split_once('-').unwrap();
    let intent=if phase!="click"{Some(dom.in_scope(ScopeId::ROOT,||RefreshIntent::capture(&original,alive.clone())).unwrap())}else{None};
    if phase=="prepare" {
        let intent=intent.unwrap();
        let mut read=Box::pin(intent.read());let fresh=in_dom(&dom,read.as_mut()).await.unwrap();
        let mut prepare=dom.in_scope(ScopeId::ROOT,||Box::pin(prepare_request(&fresh,intent.owner.clone(),&intent.original.action,None,None)));
        change(kind,&dom,&server,&alive).await;
        assert!(in_dom(&dom,prepare.as_mut()).await.is_err());
        assert_eq!(server.status_reads(),before+1);
    } else {
        change(kind,&dom,&server,&alive).await;
        if let Some(intent)=intent {let mut request=Box::pin(intent.read());assert!(in_dom(&dom,request.as_mut()).await.is_err());}
        else {assert!(dom.in_scope(ScopeId::ROOT,||RefreshIntent::capture(&original,alive.clone())).is_err());}
        assert_eq!(server.status_reads(),before,"retired callback must make zero status IO");
    }
    assert_eq!(server.posts(),0);
}
#[tokio::test(flavor="current_thread")]
async fn actual_refresh_held_status_rejects_replacements_without_prepare_or_late_authority(){
    let Some(case)=child("actual_refresh_held_status_rejects_replacements_without_prepare_or_late_authority",&["mode","auth","base","mount"])else{return;};
    let server=Server::new();let dom=incoming::dom(&server.base);incoming::mode_get(&dom).await;let original=review(&dom).await;
    let alive=mounted_alive();let intent=dom.in_scope(ScopeId::ROOT,||RefreshIntent::capture(&original,alive.clone())).unwrap();
    let before=server.status_reads();server.hold.store(true,Ordering::Release);let mut request=Box::pin(intent.read());
    let started=Instant::now();
    std::future::poll_fn(|cx|{
        assert!(started.elapsed()<Duration::from_secs(3));
        let progress=dom.in_scope(ScopeId::ROOT,||request.as_mut().poll(cx));assert!(progress.is_pending());
        if server.status_reads()>before {std::task::Poll::Ready(())}else{cx.waker().wake_by_ref();std::task::Poll::Pending}
    }).await;
    change(&case,&dom,&server,&alive).await;server.hold.store(false,Ordering::Release);
    assert!(in_dom(&dom,request.as_mut()).await.is_err());assert_eq!(server.posts(),0);
    dom.in_scope(ScopeId::ROOT,||assert!(intent.check_current().is_err()));
}

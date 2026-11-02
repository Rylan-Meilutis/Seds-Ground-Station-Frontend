//! Execute the actual vendor navigation closure and IPC handler headlessly.
//! Only the GUI window container and OS browser spawn are replaced by fixtures.
use std::{fs, process::Command};
fn body(source: &str, signature: &str) -> String {
    let start = source.find(signature).expect("actual vendor entry point");
    let open = start + source[start..].find('{').unwrap();
    let mut depth = 1;
    for (offset, c) in source[open + 1..].char_indices() {
        match c { '{' => depth += 1, '}' => depth -= 1, _ => {} }
        if depth == 0 { return source[open..open + 2 + offset].to_string(); }
    }
    panic!("unclosed vendor body")
}
#[test]
fn native_navigation_and_ipc_respect_each_originating_window_policy() {
    let app = include_str!("../vendor-dioxus-desktop/src/app.rs");
    let webview = include_str!("../vendor-dioxus-desktop/src/webview.rs");
    let ipc = body(app, "pub fn handle_browser_open(");
    let navigation = body(webview, ".with_navigation_handler(move |var|");
    let code = format!(r##"
#![allow(dead_code)]
use std::{{collections::HashMap,sync::atomic::{{AtomicBool,AtomicUsize,Ordering}}}};
#[macro_export] macro_rules! report_error {{($($arg:tt)*)=>{{let _=format_args!($($arg)*);}}}}
mod tracing {{pub use crate::report_error as error;}}
static OPENS:AtomicUsize=AtomicUsize::new(0);
mod webbrowser {{pub fn open(_: &str)->Result<(),String> {{super::OPENS.fetch_add(1,super::Ordering::SeqCst);Ok(())}}}}
type WindowId=u32;
struct View {{external_browser_enabled:bool}}
struct App {{webviews:HashMap<u32,View>}}
struct Value(String);impl Value {{fn as_str(&self)->Option<&str>{{Some(&self.0)}}}}
struct Params(Option<Value>);impl Params {{fn as_object(&self)->Option<&Self>{{Some(self)}}fn contains_key(&self,k:&str)->bool{{k=="href"&&self.0.is_some()}}fn get(&self,k:&str)->Option<&Value>{{if k=="href"{{self.0.as_ref()}}else{{None}}}}}}
struct IpcMessage(Params);impl IpcMessage {{fn params(self)->Params{{self.0}}}}
impl App {{pub fn handle_browser_open(&mut self,msg:IpcMessage,id:WindowId){ipc}}}
fn navigate(external_browser_enabled:bool,url:&str,trusted:bool)->bool {{
 let in_app_navigation_handler:Option<Box<dyn Fn(&str)->bool>>=Some(Box::new(move |_|trusted));
 let navigation_handler:Option<Box<dyn Fn(&str)->bool>>=Some(Box::new(|_|false));
 let page_loaded=AtomicBool::new(false);
 let actual=move |var:String| {navigation}; actual(url.into())
}}
fn main() {{
 for url in ["https://station.invalid/program","https://external.invalid/","http://external.invalid/","mailto:user@example.invalid"] {{
  let before=OPENS.load(Ordering::SeqCst);assert!(!navigate(false,url,false));assert_eq!(OPENS.load(Ordering::SeqCst),before);
  assert!(!navigate(true,url,false));assert_eq!(OPENS.load(Ordering::SeqCst),before+1);
 }}
 for enabled in [false,true] {{let before=OPENS.load(Ordering::SeqCst);assert!(navigate(enabled,"https://station.invalid/program",true));assert_eq!(OPENS.load(Ordering::SeqCst),before);}}
 let mut app=App{{webviews:HashMap::from([(1,View{{external_browser_enabled:false}}),(2,View{{external_browser_enabled:true}})])}};
 for id in [1,3] {{let before=OPENS.load(Ordering::SeqCst);app.handle_browser_open(IpcMessage(Params(Some(Value("https://external.invalid/".into())))),id);assert_eq!(OPENS.load(Ordering::SeqCst),before);}}
 let before=OPENS.load(Ordering::SeqCst);app.handle_browser_open(IpcMessage(Params(Some(Value("https://external.invalid/".into())))),2);assert_eq!(OPENS.load(Ordering::SeqCst),before+1);
 app.handle_browser_open(IpcMessage(Params(None)),2);assert_eq!(OPENS.load(Ordering::SeqCst),before+1);
 app.webviews.remove(&2);app.handle_browser_open(IpcMessage(Params(Some(Value("https://external.invalid/".into())))),2);assert_eq!(OPENS.load(Ordering::SeqCst),before+1);
}}
"##);
    let dir = std::env::temp_dir().join(format!("gs26-native-browser-policy-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.rs"); let binary = dir.join("policy");
    fs::write(&source, code).unwrap();
    let result = Command::new("rustc").args(["--edition=2024", "-Dwarnings"]).arg(&source).arg("-o").arg(&binary).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert!(Command::new(&binary).status().unwrap().success());
    fs::remove_dir_all(dir).unwrap();
}

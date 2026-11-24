//! Files another app hands to this one: a `.gsreplay` bundle or a
//! `.seds-replay.json` replay opened from Files, AirDrop, a share sheet, a
//! file manager double-click or the command line (docs/replay-bundle.md,
//! "Opening files from other apps").
//!
//! Platform code calls [`offer`] (or [`offer_path`]) from any thread, before
//! or after the app has mounted. The newest file waits in one slot; the
//! listener installed by [`use_opened_file_listener`] moves
//! [`OPENED_FILE_REVISION`] to match, the app root then navigates to the
//! offline replay view and that view calls [`take`]. A file over
//! [`MAX_BYTES`] is never read whole: [`take_rejection`] returns a message
//! for it instead.
use dioxus::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::task::{Poll, Waker};

/// Largest file accepted, the bundle reader's own unpacked ceiling.
pub(crate) const MAX_BYTES: usize = 192 << 20;

/// Moves once per offered file (or rejection). Readers subscribe to it and
/// then [`take`] what is waiting.
pub(crate) static OPENED_FILE_REVISION: GlobalSignal<u64> = Signal::global(|| 0);

enum Pending {
    File(String, Vec<u8>),
    Rejected(String),
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WAKER: Mutex<Option<Waker>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn too_large(name: &str) -> String {
    format!("{name} is larger than 192 MiB, the largest replay this app opens.")
}

/// Hands a file's name and contents to the app; a newer file replaces one not
/// yet taken. Safe from any thread and before the app has mounted.
// The web build has no opener (no PWA manifest or launch queue yet).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn offer(name: String, bytes: Vec<u8>) {
    if bytes.len() > MAX_BYTES {
        reject(too_large(&name));
    } else {
        deposit(Pending::File(name, bytes));
    }
}

/// Reports a file that could not be read, in the replay view.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn reject(message: String) {
    deposit(Pending::Rejected(message));
}

fn deposit(pending: Pending) {
    *lock(&PENDING) = Some(pending);
    SEQUENCE.fetch_add(1, Ordering::AcqRel);
    wake();
}

fn wake() {
    if let Some(waker) = lock(&WAKER).take() {
        waker.wake();
    }
}

/// The waiting file, once.
pub(crate) fn take() -> Option<(String, Vec<u8>)> {
    let mut slot = lock(&PENDING);
    match slot.take() {
        Some(Pending::File(name, bytes)) => Some((name, bytes)),
        other => {
            *slot = other;
            None
        }
    }
}

/// Why the last opened file was not handed over (too large, unreadable), once.
pub(crate) fn take_rejection() -> Option<String> {
    let mut slot = lock(&PENDING);
    match slot.take() {
        Some(Pending::Rejected(message)) => Some(message),
        other => {
            *slot = other;
            None
        }
    }
}

/// A file or rejection is waiting (cold start routing).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn has_pending() -> bool {
    lock(&PENDING).is_some()
}

/// Reads a file the platform named and offers it, refusing more than
/// [`MAX_BYTES`] without reading it whole.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn offer_path(name: Option<String>, path: &std::path::Path) {
    offer_path_limited(name, path, MAX_BYTES);
}

#[cfg(not(target_arch = "wasm32"))]
fn offer_path_limited(name: Option<String>, path: &std::path::Path, limit: usize) {
    use std::io::Read;
    let name = name
        .filter(|name| !name.trim().is_empty())
        .or_else(|| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "opened file".to_string());
    let result = std::fs::File::open(path).and_then(|file| {
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    match result {
        Ok(bytes) if bytes.len() > limit => reject(too_large(&name)),
        Ok(bytes) => offer(name, bytes),
        Err(error) => {
            crate::debug_log::append(&format!("[opened-file] could not read {}: {error}", path.display()));
            reject(format!("Could not read {name}: {error}"));
        }
    }
}

/// `Event::Opened` (iOS scene/app delegate, macOS `application:openURLs:`):
/// the last file URL is offered; other URLs (deep links) are ignored. On iOS
/// the vendored Tao already started security-scoped access for file URLs.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn offer_urls(urls: &[url::Url]) {
    let Some(path) = urls.iter().rev().filter(|url| url.scheme() == "file").find_map(|url| url.to_file_path().ok()) else {
        return;
    };
    crate::debug_log::append(&format!("[opened-file] opened by the system: {}", path.display()));
    offer_path(None, &path);
}

/// Desktop launch arguments: the first argument that names an existing file
/// (a double-clicked `.gsreplay`, or `groundstation_frontend path.gsreplay`).
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub(crate) fn offer_launch_arguments() {
    let path = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .find(|path| !path.to_string_lossy().starts_with('-') && path.is_file());
    if let Some(path) = path {
        crate::debug_log::append(&format!("[opened-file] launch argument: {}", path.display()));
        offer_path(None, &path);
    }
}

/// Keeps [`OPENED_FILE_REVISION`] in step with offered files. Mounted once,
/// in the app root; offers from other threads wake it.
pub(crate) fn use_opened_file_listener() {
    use_future(|| async {
        let mut seen = 0;
        loop {
            let sequence = std::future::poll_fn(|cx| {
                *lock(&WAKER) = Some(cx.waker().clone());
                #[cfg(target_os = "android")]
                android::pull();
                let sequence = SEQUENCE.load(Ordering::Acquire);
                if sequence == seen { Poll::Pending } else { Poll::Ready(sequence) }
            })
            .await;
            seen = sequence;
            *OPENED_FILE_REVISION.write() = sequence;
        }
    });
}

/// Android: `com.ubseds.gs26.OpenedFileBridge` copies an ACTION_VIEW /
/// ACTION_SEND content URI into the app cache and signals here; the listener
/// then pulls the copy through JNI.
#[cfg(target_os = "android")]
mod android {
    use ::jni::objects::{JClass, JObject, JString, JValue};
    use ::jni::{EnvUnowned, JavaVM, jni_sig, jni_str};
    use ndk_context::android_context;
    use std::sync::atomic::{AtomicBool, Ordering};

    const CLASS: &str = "com.ubseds.gs26.OpenedFileBridge";
    /// True at start so a file that launched the app is pulled.
    static SIGNALLED: AtomicBool = AtomicBool::new(true);

    /// OpenedFileBridge finished copying a file (or failed to).
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_ubseds_gs26_OpenedFileBridge_nativeOnOpenedFile(
        _env: EnvUnowned<'_>,
        _class: JClass<'_>,
    ) {
        SIGNALLED.store(true, Ordering::Release);
        super::wake();
    }

    /// `OpenedFileBridge.takePending()`: "name\npath\nerror", or null.
    fn take_pending() -> Option<String> {
        let ctx = android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
        vm.attach_current_thread(|env| -> ::jni::errors::Result<Option<String>> {
            let context = unsafe { JObject::from_raw(env, ctx.context().cast()) };
            let result = (|| {
                let loader = env
                    .call_method(&context, jni_str!("getClassLoader"), jni_sig!("()Ljava/lang/ClassLoader;"), &[])
                    .and_then(|v| v.l())
                    .ok()?;
                let name: JString<'_> = env.new_string(CLASS).ok()?;
                let class = env
                    .call_method(
                        &loader,
                        jni_str!("loadClass"),
                        jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
                        &[JValue::Object(&JObject::from(name))],
                    )
                    .and_then(|v| v.l())
                    .ok()?;
                let class = unsafe { JClass::from_raw(env, class.into_raw().cast()) };
                let value = env
                    .call_static_method(&class, jni_str!("takePending"), jni_sig!("()Ljava/lang/String;"), &[])
                    .and_then(|v| v.l())
                    .ok()?;
                if value.is_null() {
                    return None;
                }
                env.as_cast::<JString>(&value).ok()?.try_to_string(env).ok()
            })();
            if env.exception_check() {
                let _ = env.exception_clear();
            }
            let _ = context.into_raw();
            Ok(result)
        })
        .ok()
        .flatten()
    }

    pub(super) fn pull() {
        if !SIGNALLED.swap(false, Ordering::AcqRel) {
            return;
        }
        let Some(raw) = take_pending() else { return };
        let mut parts = raw.splitn(3, '\n');
        let name = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default();
        let error = parts.next().unwrap_or_default();
        if !error.is_empty() {
            super::reject(error.to_string());
        } else if !path.is_empty() {
            let path = std::path::Path::new(path);
            super::offer_path(Some(name), path);
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The slot is process-wide; tests take turns.
    static TURN: Mutex<()> = Mutex::new(());

    #[test]
    fn newest_offer_is_taken_once() {
        let _turn = lock(&TURN);
        let before = SEQUENCE.load(Ordering::Acquire);
        offer("a.gsreplay".into(), vec![1]);
        offer("b.gsreplay".into(), vec![2, 3]);
        assert_eq!(SEQUENCE.load(Ordering::Acquire), before + 2);
        assert!(has_pending());
        assert_eq!(take_rejection(), None, "a file is not a rejection");
        assert_eq!(take(), Some(("b.gsreplay".to_string(), vec![2, 3])));
        assert_eq!(take(), None);
        assert!(!has_pending());
    }

    #[test]
    fn oversized_file_becomes_a_message() {
        let _turn = lock(&TURN);
        let dir = std::env::temp_dir().join(format!("gs26-opened-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.gsreplay");
        std::fs::write(&path, vec![0u8; 33]).unwrap();
        offer_path_limited(None, &path, 32);
        assert_eq!(take(), None);
        let message = take_rejection().expect("rejection");
        assert!(message.contains("big.gsreplay") && message.contains("192 MiB"), "{message}");

        offer_path_limited(Some("named.seds-replay.json".into()), &path, 33);
        assert_eq!(take(), Some(("named.seds-replay.json".to_string(), vec![0u8; 33])));

        offer_path(None, &dir.join("missing.gsreplay"));
        assert!(take_rejection().expect("unreadable").starts_with("Could not read missing.gsreplay"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_urls_are_offered_and_deep_links_ignored() {
        let _turn = lock(&TURN);
        let dir = std::env::temp_dir().join(format!("gs26-opened-url-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("flight.gsreplay");
        std::fs::write(&path, b"PK").unwrap();
        let before = SEQUENCE.load(Ordering::Acquire);
        offer_urls(&[url::Url::parse("gs26://station").unwrap()]);
        assert_eq!(SEQUENCE.load(Ordering::Acquire), before);
        offer_urls(&[url::Url::from_file_path(&path).unwrap()]);
        assert_eq!(take(), Some(("flight.gsreplay".to_string(), b"PK".to_vec())));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

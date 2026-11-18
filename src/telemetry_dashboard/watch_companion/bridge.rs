//! Platform transport to the paired watch. iOS uses WatchConnectivity
//! (`assets/WatchBridge.m`), Android the Wear Data Layer
//! (`platform/android/kotlin/com/ubseds/gs26/WatchBridge.kt`). Elsewhere the
//! bridge is absent and every call is a no-op.

/// Link state reported by the native side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkState {
    /// This phone platform has a watch bridge at all.
    pub supported: bool,
    /// A watch is paired (iOS) or a Wear node is connected (Android).
    pub paired: bool,
    /// The companion app is installed on the watch (iOS only; Android
    /// reports true once a node advertising `gs26_watch` is seen).
    pub installed: bool,
    /// Messages can be delivered right now.
    pub reachable: bool,
    /// The native session finished activating (iOS WCSession; Android sets
    /// it once the Data Layer answered). Until then `paired` and
    /// `installed` are unknown and read as false.
    pub activated: bool,
    /// The raw bits, for diagnostics.
    pub bits: i32,
}

impl LinkState {
    pub fn from_bits(bits: i32) -> Self {
        Self {
            supported: bits & 1 != 0,
            paired: bits & 2 != 0,
            installed: bits & 4 != 0,
            reachable: bits & 8 != 0,
            activated: bits & 16 != 0,
            bits,
        }
    }

    pub fn summary(self) -> &'static str {
        match (self.supported, self.paired, self.installed, self.reachable) {
            (false, ..) => "Watch apps pair with the iOS and Android apps",
            (true, false, ..) if !self.activated => "Connecting to the watch service…",
            (true, false, ..) => "No paired watch",
            (true, true, false, _) => "Watch app not installed",
            (true, true, true, false) => "Watch app installed, not open",
            (true, true, true, true) => "Watch connected",
        }
    }

    /// One diagnostic line: the raw bits and what each means.
    pub fn debug_text(self) -> String {
        let flag = |on: bool, yes: &str, no: &str| if on { yes.to_owned() } else { no.to_owned() };
        format!(
            "bits 0x{:02x}: {}, {}, {}, {}, {}",
            self.bits,
            flag(self.supported, "bridge", "no bridge"),
            flag(self.activated, "activated", "not activated"),
            flag(self.paired, "paired", "not paired"),
            flag(self.installed, "app installed", "app not installed"),
            flag(self.reachable, "reachable", "not reachable"),
        )
    }
}

#[cfg(target_os = "ios")]
mod imp {
    use std::ffi::{CString, c_char, c_int};
    unsafe extern "C" {
        fn gs26_watch_start();
        fn gs26_watch_publish(json: *const c_char, urgent: c_int);
        fn gs26_watch_take_incoming(buf: *mut c_char, cap: c_int) -> c_int;
        fn gs26_watch_state() -> c_int;
    }
    pub fn start() {
        unsafe { gs26_watch_start() }
    }
    pub fn publish(json: &str, urgent: bool) {
        if let Ok(c) = CString::new(json) {
            unsafe { gs26_watch_publish(c.as_ptr(), urgent as c_int) }
        }
    }
    pub fn take_incoming() -> Option<String> {
        let mut buf = vec![0u8; super::super::protocol::MAX_MESSAGE_BYTES + 1];
        let n = unsafe { gs26_watch_take_incoming(buf.as_mut_ptr().cast(), buf.len() as c_int) };
        if n <= 0 {
            return None;
        }
        buf.truncate(n as usize);
        String::from_utf8(buf).ok()
    }
    pub fn state_bits() -> i32 {
        unsafe { gs26_watch_state() }
    }
}

#[cfg(target_os = "android")]
mod imp {
    use ::jni::objects::{JClass, JObject, JString, JValue};
    use ::jni::signature::RuntimeMethodSignature;
    use ::jni::strings::JNIString;
    use ::jni::{Env, JavaVM};
    use ndk_context::android_context;

    const CLASS: &str = "com.ubseds.gs26.WatchBridge";

    /// Calls a static method on `WatchBridge`, prepending the app Context
    /// when the signature starts with one. Returns None on any JNI failure.
    fn call<R>(
        method: &str,
        sig: &str,
        args: &[JValue<'_>],
        read: impl for<'a> FnOnce(&mut Env<'a>, ::jni::objects::JValueOwned<'a>) -> Option<R>,
    ) -> Option<R> {
        let ctx = android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
        vm.attach_current_thread(|env| -> ::jni::errors::Result<Option<R>> {
            let context = unsafe { JObject::from_raw(env, ctx.context().cast()) };
            let result = (|| {
                let parsed = RuntimeMethodSignature::from_str(sig).ok()?;
                let loader = env
                    .call_method(
                        &context,
                        ::jni::jni_str!("getClassLoader"),
                        ::jni::jni_sig!("()Ljava/lang/ClassLoader;"),
                        &[],
                    )
                    .and_then(|v| v.l())
                    .ok()?;
                let name: JString<'_> = env.new_string(CLASS).ok()?;
                let class = env
                    .call_method(
                        &loader,
                        ::jni::jni_str!("loadClass"),
                        ::jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
                        &[JValue::Object(&JObject::from(name))],
                    )
                    .and_then(|v| v.l())
                    .ok()?;
                let class = unsafe { JClass::from_raw(env, class.into_raw().cast()) };
                let mut all = Vec::with_capacity(args.len() + 1);
                if sig.starts_with("(Landroid/content/Context;") {
                    all.push(JValue::Object(&context));
                }
                all.extend_from_slice(args);
                let value = env
                    .call_static_method(
                        &class,
                        JNIString::from(method),
                        parsed.method_signature(),
                        &all,
                    )
                    .ok()?;
                read(env, value)
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

    pub fn start() {
        let _ = call(
            "start",
            "(Landroid/content/Context;)V",
            &[],
            |_, _| Some(()),
        );
    }
    pub fn publish(json: &str, urgent: bool) {
        let ctx = android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
        // The string must be created inside an attached env.
        let json = json.to_owned();
        let _ = vm.attach_current_thread(|env| -> ::jni::errors::Result<()> {
            let s = env.new_string(&json)?;
            let obj = JObject::from(s);
            let _ = call(
                "publish",
                "(Ljava/lang/String;Z)V",
                &[JValue::Object(&obj), JValue::Bool(urgent)],
                |_, _| Some(()),
            );
            Ok(())
        });
    }
    pub fn take_incoming() -> Option<String> {
        call("takeIncoming", "()Ljava/lang/String;", &[], |env, value| {
            let obj = value.l().ok()?;
            if obj.is_null() {
                return None;
            }
            let s = unsafe { JString::from_raw(env, obj.into_raw()) };
            s.try_to_string(env).ok()
        })
    }
    pub fn state_bits() -> i32 {
        call("state", "()I", &[], |_, value| value.i().ok()).unwrap_or(0)
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod imp {
    pub fn start() {}
    pub fn publish(_json: &str, _urgent: bool) {}
    pub fn take_incoming() -> Option<String> {
        None
    }
    pub fn state_bits() -> i32 {
        0
    }
}

pub fn start() {
    imp::start();
}

pub fn publish(json: &str, urgent: bool) {
    imp::publish(json, urgent);
}

/// Drains at most `max` queued watch messages.
pub fn take_incoming(max: usize) -> Vec<String> {
    (0..max).map_while(|_| imp::take_incoming()).collect()
}

pub fn link_state() -> LinkState {
    LinkState::from_bits(imp::state_bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_state_bits_and_summaries() {
        assert_eq!(LinkState::from_bits(0), LinkState::default());
        let all = LinkState::from_bits(31);
        assert!(all.supported && all.activated && all.paired && all.installed && all.reachable);
        assert_eq!(all.summary(), "Watch connected");
        // Before WCSession activation completes only the bridge bit is set.
        assert_eq!(
            LinkState::from_bits(1).summary(),
            "Connecting to the watch service…"
        );
        assert_eq!(LinkState::from_bits(17).summary(), "No paired watch");
        assert_eq!(
            LinkState::from_bits(19).summary(),
            "Watch app not installed"
        );
        assert_eq!(
            LinkState::from_bits(23).summary(),
            "Watch app installed, not open"
        );
        assert_eq!(
            LinkState::from_bits(23).debug_text(),
            "bits 0x17: bridge, activated, paired, app installed, not reachable"
        );
        assert_eq!(
            LinkState::from_bits(1).debug_text(),
            "bits 0x01: bridge, not activated, not paired, app not installed, not reachable"
        );
        // Desktop/web builds have no bridge.
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            assert!(!link_state().supported);
            assert!(take_incoming(4).is_empty());
        }
    }
}

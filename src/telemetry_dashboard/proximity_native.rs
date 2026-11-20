//! Platform glue for the proximity lock (docs/proximity-lock.md).
//!
//! * iOS: `assets/ProximityShim.m`: its own CLLocationManager with "Always"
//!   (or "When In Use") authorization, background location updates while
//!   participating, significant-change monitoring, and a CLCircularRegion
//!   around the reference that wakes the app on entry/exit.
//! * Android: `ProximityBridge` / `ProximityService` (Kotlin): a foreground
//!   service of type `location` that keeps this process (and so this
//!   reporter) alive in the background, fused location updates into the
//!   same fix the map uses, and a Geofencing API fence that wakes it.
//! * macOS, Windows, Linux: the dashboard's own location, foreground only.
//! * Web: the browser's Geolocation API while the page is open (see
//!   `proximity_lock_view`), foreground only.
//!
//! Coordinates handed to the native layers (the reference point and the
//! device's fix) stay on the device: the native code never sends them.
#![allow(dead_code)]

use super::proximity_lock::{OwnFix, Permission};

/// A background fence around the reference point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fence {
    pub lat: f64,
    pub lon: f64,
    /// The OS fence radius: the lockout radius plus the exit margin, so the
    /// OS wakes the app before and after the in-app decision changes.
    pub radius_m: f64,
}

impl Fence {
    /// The same fence, give or take `tolerance_m` of movement.
    pub(crate) fn close_to(&self, other: &Fence, tolerance_m: f64) -> bool {
        let d = super::watch_companion::geodesy::distance_m(
            super::watch_companion::geodesy::LatLon::new(self.lat, self.lon),
            super::watch_companion::geodesy::LatLon::new(other.lat, other.lon),
        );
        d.is_some_and(|d| d <= tolerance_m) && (self.radius_m - other.radius_m).abs() < 1.0
    }
}

#[cfg(target_os = "ios")]
mod imp {
    use super::*;
    use std::ffi::c_int;
    unsafe extern "C" {
        fn gs26_proximity_auth_status() -> c_int;
        fn gs26_proximity_request(always: c_int);
        fn gs26_proximity_start(lat: f64, lon: f64, radius_m: f64);
        fn gs26_proximity_stop();
        fn gs26_proximity_take_wake() -> c_int;
        fn gs26_proximity_last_fix(lat: *mut f64, lon: *mut f64, accuracy_m: *mut f64, unix_s: *mut f64) -> c_int;
        fn gs26_proximity_open_settings();
    }
    /// CLAuthorizationStatus: 0 not determined, 1 restricted, 2 denied,
    /// 3 always, 4 when in use. 5: location services off.
    pub fn permission() -> Permission {
        match unsafe { gs26_proximity_auth_status() } {
            0 => Permission::NotDetermined,
            1 | 2 | 5 => Permission::Denied,
            3 => Permission::Background,
            4 => Permission::Foreground,
            _ => Permission::NotDetermined,
        }
    }
    pub fn request_permission(background: bool) {
        unsafe { gs26_proximity_request(background as c_int) }
    }
    pub fn own_fix() -> Option<OwnFix> {
        let (mut lat, mut lon, mut acc, mut at) = (f64::NAN, f64::NAN, f64::NAN, f64::NAN);
        if unsafe { gs26_proximity_last_fix(&mut lat, &mut lon, &mut acc, &mut at) } == 0 {
            return None;
        }
        (lat.is_finite() && lon.is_finite() && at.is_finite()).then(|| OwnFix {
            lat,
            lon,
            accuracy_m: acc.is_finite().then_some(acc).filter(|a| *a >= 0.0),
            at_ms: (at * 1000.0) as i64,
        })
    }
    pub fn set_background(fence: Option<Fence>) {
        match fence {
            Some(f) => unsafe { gs26_proximity_start(f.lat, f.lon, f.radius_m) },
            None => unsafe { gs26_proximity_stop() },
        }
    }
    pub fn take_wake() -> bool {
        unsafe { gs26_proximity_take_wake() != 0 }
    }
    pub fn open_settings() {
        unsafe { gs26_proximity_open_settings() }
    }
}

#[cfg(target_os = "android")]
mod imp {
    use super::*;
    use ::jni::objects::{JClass, JObject, JString, JValue};
    use ::jni::signature::RuntimeMethodSignature;
    use ::jni::strings::JNIString;
    use ::jni::sys::jboolean;
    use ::jni::{Env, EnvUnowned, JavaVM};
    use ndk_context::android_context;
    use std::sync::atomic::{AtomicBool, Ordering};

    const CLASS: &str = "com.ubseds.gs26.ProximityBridge";
    static WAKE: AtomicBool = AtomicBool::new(false);

    /// Calls a static method on `ProximityBridge`, prepending the app
    /// Context when the signature starts with one. None on any JNI failure.
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
                    .call_static_method(&class, JNIString::from(method), parsed.method_signature(), &all)
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

    /// ProximityBridge.permissionState: 0 not asked, 1 denied, 2 while in
    /// use, 3 all the time, 4 no location hardware.
    pub fn permission() -> Permission {
        match call("permissionState", "(Landroid/content/Context;)I", &[], |_, v| v.i().ok()) {
            Some(1) => Permission::Denied,
            Some(2) => Permission::Foreground,
            Some(3) => Permission::Background,
            Some(4) => Permission::Unsupported,
            _ => Permission::NotDetermined,
        }
    }
    pub fn request_permission(background: bool) {
        let _ = call(
            "requestPermission",
            "(Landroid/content/Context;Z)V",
            &[JValue::Bool(background)],
            |_, _| Some(()),
        );
    }
    pub fn own_fix() -> Option<OwnFix> {
        let fix = crate::telemetry_dashboard::gps_android::recent_fix(
            super::super::proximity_lock::OWN_FIX_FRESH_MS as u64,
        )?;
        let at_ms = crate::telemetry_dashboard::gps_android::latest_fix_time_ms()?;
        Some(OwnFix { lat: fix.latitude, lon: fix.longitude, accuracy_m: fix.accuracy_m, at_ms })
    }
    pub fn set_background(fence: Option<Fence>) {
        let _ = match fence {
            Some(f) => call(
                "start",
                "(Landroid/content/Context;DDF)V",
                &[JValue::Double(f.lat), JValue::Double(f.lon), JValue::Float(f.radius_m as f32)],
                |_, _| Some(()),
            ),
            None => call("stop", "(Landroid/content/Context;)V", &[], |_, _| Some(())),
        };
    }
    pub fn take_wake() -> bool {
        WAKE.swap(false, Ordering::AcqRel)
    }
    pub fn open_settings() {
        let _ = call("openSettings", "(Landroid/content/Context;)V", &[], |_, _| Some(()));
    }

    /// A Geofencing API transition (ProximityGeofenceReceiver): evaluate now.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_ubseds_gs26_ProximityBridge_nativeOnGeofence(
        _env: EnvUnowned<'_>,
        _class: JClass<'_>,
        _inside: jboolean,
    ) {
        WAKE.store(true, Ordering::Release);
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod imp {
    use super::*;
    /// Desktop and web: the view supplies the fix; there is no background.
    pub fn permission() -> Permission {
        Permission::NotDetermined
    }
    pub fn request_permission(_background: bool) {}
    pub fn own_fix() -> Option<OwnFix> {
        None
    }
    pub fn set_background(_fence: Option<Fence>) {}
    pub fn take_wake() -> bool {
        false
    }
    pub fn open_settings() {}
}

/// True on iOS and Android, where the native layer owns permission, the fix
/// and background operation.
pub(crate) const NATIVE_MOBILE: bool = cfg!(any(target_os = "ios", target_os = "android"));

pub(crate) fn permission() -> Permission {
    imp::permission()
}
pub(crate) fn request_permission(background: bool) {
    imp::request_permission(background)
}
pub(crate) fn own_fix() -> Option<OwnFix> {
    imp::own_fix()
}
pub(crate) fn set_background(fence: Option<Fence>) {
    imp::set_background(fence)
}
pub(crate) fn take_wake() -> bool {
    imp::take_wake()
}
pub(crate) fn open_settings() {
    imp::open_settings()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fences_compare_with_a_movement_tolerance() {
        let a = Fence { lat: 42.0, lon: -78.0, radius_m: 180.0 };
        let moved = Fence { lat: 42.0001, lon: -78.0, ..a };
        assert!(a.close_to(&moved, 25.0), "11 m of rocket GPS jitter keeps the fence");
        assert!(!a.close_to(&Fence { lat: 42.001, ..a }, 25.0));
        assert!(!a.close_to(&Fence { radius_m: 300.0, ..a }, 25.0));
        assert_eq!(NATIVE_MOBILE, cfg!(any(target_os = "ios", target_os = "android")));
    }
}

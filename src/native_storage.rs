#[cfg(all(not(target_arch = "wasm32"), target_os = "android"))]
/// Resolves the Android app-private files directory through JNI.
pub(crate) fn android_files_dir() -> Option<std::path::PathBuf> {
    use ::jni::objects::{JObject, JString};
    use ::jni::{JavaVM, jni_sig, jni_str};
    use ndk_context::android_context;

    let ctx = android_context();
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
    vm.attach_current_thread(|env| -> ::jni::errors::Result<std::path::PathBuf> {
        let context = unsafe { JObject::from_raw(env, ctx.context().cast()) };

        let files_dir = env
            .call_method(
                &context,
                jni_str!("getFilesDir"),
                jni_sig!("()Ljava/io/File;"),
                &[],
            )?
            .l()?;
        let path_obj = env
            .call_method(
                &files_dir,
                jni_str!("getAbsolutePath"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let path = env.as_cast::<JString>(&path_obj)?.try_to_string(env)?;

        let _ = context.into_raw();
        Ok(std::path::PathBuf::from(path))
    })
    .ok()
}

#[cfg(not(target_arch = "wasm32"))]
mod file;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use file::{atomic_write, remove_durable, transaction_lock};

/// Report the operation and OS error category only, never stored values/tokens.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn report_error(operation: &'static str, error: &std::io::Error) {
    crate::debug_log::append(&format!("[storage] {operation} failed: {:?}", error.kind()));
}

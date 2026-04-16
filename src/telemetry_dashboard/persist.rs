/// Reads a persisted string value from browser storage or the native JSON store.
pub fn get_string(key: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        use web_sys::window;
        let w = window()?;
        let ls = w.local_storage().ok()??;
        ls.get_item(key).ok().flatten()
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        native::get_string(key).ok().flatten()
    }
}

/// Persists a string value across app launches.
pub fn set_string(key: &str, value: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        use web_sys::window;
        if let Some(w) = window()
            && let Ok(Some(ls)) = w.local_storage()
        {
            let _ = ls.set_item(key, value);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = native::set_string(key, value);
    }
}

/// Removes a persisted key when the current platform supports it.
pub fn _remove(key: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        use web_sys::window;
        if let Some(w) = window()
            && let Ok(Some(ls)) = w.local_storage()
        {
            let _ = ls.remove_item(key);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = native::_remove(key);
    }
}

/// Reads a stored string value or falls back to the provided default.
pub fn get_or(key: &str, default: &str) -> String {
    get_string(key).unwrap_or_else(|| default.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::HashMap;
    use std::io;

    /// Resolves the default native storage root when no platform-specific path is available.
    fn fallback_storage_base_dir() -> std::path::PathBuf {
        dirs::data_local_dir()
            .or_else(dirs::data_dir)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()))
    }

    #[cfg(target_os = "android")]
    /// Resolves the Android app-private storage root through JNI.
    fn android_storage_base_dir() -> Option<std::path::PathBuf> {
        crate::native_storage::android_files_dir()
    }

    /// Picks the best native storage root for the JSON persistence file.
    fn storage_base_dir() -> std::path::PathBuf {
        #[cfg(target_os = "android")]
        {
            if let Some(path) = android_storage_base_dir() {
                return path;
            }
        }

        fallback_storage_base_dir()
    }

    /// Returns the full path to the native JSON persistence file.
    fn storage_path() -> std::path::PathBuf {
        let mut base = storage_base_dir();
        base.push("gs26");
        base.push("storage.json");
        base
    }

    /// Loads the native persistence map from disk.
    fn load_map() -> Result<HashMap<String, String>, io::Error> {
        let path = storage_path();
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
            Err(e) => return Err(e),
        };

        let map = serde_json::from_slice::<HashMap<String, String>>(&bytes).unwrap_or_default();
        Ok(map)
    }

    /// Saves the native persistence map back to disk.
    fn save_map(map: &HashMap<String, String>) -> Result<(), io::Error> {
        let path = storage_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(map).unwrap_or_else(|_| b"{}".to_vec());
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Reads a string key from the native persistence file.
    pub fn get_string(key: &str) -> Result<Option<String>, io::Error> {
        let map = load_map()?;
        Ok(map.get(key).cloned())
    }

    /// Writes a string key into the native persistence file.
    pub fn set_string(key: &str, value: &str) -> Result<(), io::Error> {
        let mut map = load_map()?;
        map.insert(key.to_string(), value.to_string());
        save_map(&map)?;
        Ok(())
    }

    /// Removes a key from the native persistence file.
    pub fn _remove(key: &str) -> Result<(), io::Error> {
        let mut map = load_map()?;
        map.remove(key);
        save_map(&map)?;
        Ok(())
    }
}

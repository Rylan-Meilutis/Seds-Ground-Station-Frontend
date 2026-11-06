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
        match native::get_string(key) {
            Ok(value) => value,
            Err(error) => {
                crate::native_storage::report_error("read frontend preferences", &error);
                None
            }
        }
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
        if let Err(error) = native::set_string(key, value) {
            crate::native_storage::report_error("save frontend preferences", &error);
        }
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

/// Removes all persisted keys that share the provided prefix.
pub fn remove_prefix(prefix: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        use web_sys::window;
        if let Some(w) = window()
            && let Ok(Some(ls)) = w.local_storage()
        {
            let len = ls.length().ok().unwrap_or(0);
            let mut keys = Vec::new();
            for idx in 0..len {
                if let Ok(Some(key)) = ls.key(idx)
                    && key.starts_with(prefix)
                {
                    keys.push(key);
                }
            }
            for key in keys {
                let _ = ls.remove_item(&key);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = native::remove_prefix(prefix);
    }
}

/// Removes all persisted app keys from the current storage backend.
pub fn clear_all() {
    #[cfg(target_arch = "wasm32")]
    {
        use web_sys::window;
        if let Some(w) = window()
            && let Ok(Some(ls)) = w.local_storage()
        {
            let _ = ls.clear();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = native::clear_all();
    }
}

/// Reads a stored string value or falls back to the provided default.
pub fn get_or(key: &str, default: &str) -> String {
    get_string(key).unwrap_or_else(|| default.to_string())
}

/// Returns the UTF-8 byte size of a persisted string value.
pub fn byte_len(key: &str) -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        get_string(key).map(|value| value.len()).unwrap_or(0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match native::byte_len(key) {
            Ok(length) => length,
            Err(error) => {
                crate::native_storage::report_error("read frontend preferences", &error);
                0
            }
        }
    }
}

/// Returns the combined UTF-8 byte size of values whose keys share a prefix.
pub fn byte_len_prefix(prefix: &str) -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        use web_sys::window;
        let Some(w) = window() else {
            return 0;
        };
        let Ok(Some(ls)) = w.local_storage() else {
            return 0;
        };
        let len = ls.length().ok().unwrap_or(0);
        let mut total = 0usize;
        for idx in 0..len {
            if let Ok(Some(key)) = ls.key(idx)
                && key.starts_with(prefix)
                && let Ok(Some(value)) = ls.get_item(&key)
            {
                total = total.saturating_add(value.len());
            }
        }
        total
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        native::byte_len_prefix(prefix).ok().unwrap_or(0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::HashMap;
    use std::io;
    use std::sync::Mutex;

    static STORE: std::sync::LazyLock<Mutex<CachedStore>> = std::sync::LazyLock::new(|| {
        Mutex::new(CachedStore {
            path: None,
            identity: None,
            map: HashMap::new(),
            #[cfg(test)]
            reloads: 0,
        })
    });

    /// Identity includes inode/change time on Unix, so same-size atomic replacements
    /// and in-place edits invalidate the cache. Every operation still takes the
    /// process-shared transaction lock; another app process cannot lose its writes.
    #[derive(PartialEq, Eq)]
    struct FileIdentity {
        len: u64,
        modified: Option<std::time::SystemTime>,
        created: Option<std::time::SystemTime>,
        #[cfg(unix)]
        unix: (u64, u64, i64, i64),
    }
    fn file_identity(path: &std::path::Path) -> io::Result<Option<FileIdentity>> {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Some(FileIdentity {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
            #[cfg(unix)]
            unix: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        }))
    }
    struct CachedStore {
        path: Option<std::path::PathBuf>,
        identity: Option<FileIdentity>,
        map: HashMap<String, String>,
        #[cfg(test)]
        reloads: usize,
    }
    impl CachedStore {
        fn invalidate(&mut self) {
            self.path = None;
        }
        fn refresh(&mut self, path: &std::path::Path) -> io::Result<()> {
            let identity = file_identity(path)?;
            if self.path.as_deref() == Some(path) && self.identity == identity {
                return Ok(());
            }
            self.invalidate();
            // Cooperating writers hold our sidecar lock. Also fail closed if an
            // external editor repeatedly replaces the file during the read.
            let mut before = identity;
            for _ in 0..3 {
                let map = load_map(path)?;
                #[cfg(test)]
                {
                    self.reloads += 1;
                }
                let after = file_identity(path)?;
                if before == after {
                    self.map = map;
                    self.identity = after;
                    self.path = Some(path.to_owned());
                    return Ok(());
                }
                before = after;
            }
            Err(io::Error::other(
                "frontend preferences changed while reading",
            ))
        }
        fn read<T>(
            &mut self,
            path: &std::path::Path,
            read: impl FnOnce(&HashMap<String, String>) -> T,
        ) -> io::Result<T> {
            let _file_lock = crate::native_storage::transaction_lock(path)?;
            self.refresh(path)?;
            Ok(read(&self.map))
        }
        fn update(
            &mut self,
            path: &std::path::Path,
            update: impl FnOnce(&mut HashMap<String, String>) -> bool,
        ) -> io::Result<()> {
            self.update_with(path, update, save_map)
        }
        fn update_with(
            &mut self,
            path: &std::path::Path,
            update: impl FnOnce(&mut HashMap<String, String>) -> bool,
            save: impl FnOnce(&std::path::Path, &HashMap<String, String>) -> io::Result<()>,
        ) -> io::Result<()> {
            let _file_lock = crate::native_storage::transaction_lock(path)?;
            self.refresh(path)?;
            if !update(&mut self.map) {
                return Ok(());
            }
            // Never serve an in-memory mutation when durable replacement failed.
            self.invalidate();
            save(path, &self.map)
        }
    }
    fn store() -> io::Result<std::sync::MutexGuard<'static, CachedStore>> {
        STORE
            .lock()
            .map_err(|_| io::Error::other("frontend preference lock poisoned"))
    }

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
    fn load_map(path: &std::path::Path) -> Result<HashMap<String, String>, io::Error> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
            Err(e) => return Err(e),
        };

        if bytes.is_empty() {
            return Ok(HashMap::new());
        }
        serde_json::from_slice::<HashMap<String, String>>(&bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid frontend preference JSON",
            )
        })
    }

    /// Saves the native persistence map back to disk.
    fn save_map(path: &std::path::Path, map: &HashMap<String, String>) -> Result<(), io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(map).map_err(io::Error::other)?;
        crate::native_storage::atomic_write(path, &bytes)?;
        Ok(())
    }

    /// Reads just the requested value; the parsed map is shared until the file changes.
    pub fn get_string(key: &str) -> Result<Option<String>, io::Error> {
        store()?.read(&storage_path(), |map| map.get(key).cloned())
    }
    pub fn byte_len(key: &str) -> Result<usize, io::Error> {
        store()?.read(&storage_path(), |map| map.get(key).map_or(0, String::len))
    }
    pub fn set_string(key: &str, value: &str) -> Result<(), io::Error> {
        store()?.update(&storage_path(), |map| {
            if map.get(key).is_some_and(|old| old == value) {
                return false;
            }
            map.insert(key.to_owned(), value.to_owned());
            true
        })
    }
    pub fn _remove(key: &str) -> Result<(), io::Error> {
        store()?.update(&storage_path(), |map| map.remove(key).is_some())
    }
    pub fn remove_prefix(prefix: &str) -> Result<(), io::Error> {
        store()?.update(&storage_path(), |map| {
            let before = map.len();
            map.retain(|key, _| !key.starts_with(prefix));
            map.len() != before
        })
    }
    pub fn byte_len_prefix(prefix: &str) -> Result<usize, io::Error> {
        store()?.read(&storage_path(), |map| {
            map.iter()
                .filter(|(key, _)| key.starts_with(prefix))
                .map(|(_, value)| value.len())
                .sum()
        })
    }
    pub fn clear_all() -> Result<(), io::Error> {
        let mut store = store()?;
        let path = storage_path();
        let _file_lock = crate::native_storage::transaction_lock(&path)?;
        store.invalidate();
        crate::native_storage::remove_durable(&path)
    }

    #[cfg(test)]
    mod cache_tests {
        use super::*;
        struct Fixture {
            directory: std::path::PathBuf,
            path: std::path::PathBuf,
            cache: CachedStore,
        }
        impl Fixture {
            fn new() -> Self {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let directory = std::env::temp_dir().join(format!(
                    "gs26-persist-cache-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                std::fs::create_dir_all(&directory).unwrap();
                Self {
                    path: directory.join("storage.json"),
                    directory,
                    cache: CachedStore {
                        path: None,
                        identity: None,
                        map: HashMap::new(),
                        reloads: 0,
                    },
                }
            }
            fn get(&mut self, key: &str) -> io::Result<Option<String>> {
                self.cache.read(&self.path, |map| map.get(key).cloned())
            }
            fn replace(&self, value: &str) {
                save_map(&self.path, &HashMap::from([("key".into(), value.into())])).unwrap();
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.directory);
            }
        }
        #[test]
        fn unchanged_large_store_reads_parse_once_and_measure_without_clone() {
            let mut f = Fixture::new();
            let large = "x".repeat(660_000);
            save_map(
                &f.path,
                &HashMap::from([("key".into(), "on".into()), ("telemetry".into(), large)]),
            )
            .unwrap();
            for _ in 0..1000 {
                assert_eq!(f.get("key").unwrap().as_deref(), Some("on"));
                assert_eq!(
                    f.cache.read(&f.path, |map| map["telemetry"].len()).unwrap(),
                    660_000
                );
            }
            assert_eq!(f.cache.reloads, 1);
        }
        #[test]
        fn external_same_size_replace_in_place_delete_and_recreate_invalidate() {
            let mut f = Fixture::new();
            f.replace("old");
            assert_eq!(f.get("key").unwrap().as_deref(), Some("old"));
            f.replace("new"); // Same size, new inode.
            assert_eq!(f.get("key").unwrap().as_deref(), Some("new"));
            let before = std::fs::read_to_string(&f.path).unwrap();
            let edited = before.replace("new", "alt");
            assert_eq!(edited.len(), before.len());
            std::fs::write(&f.path, edited).unwrap();
            assert_eq!(f.get("key").unwrap().as_deref(), Some("alt"));
            std::fs::remove_file(&f.path).unwrap();
            assert_eq!(f.get("key").unwrap(), None);
            f.replace("again");
            assert_eq!(f.get("key").unwrap().as_deref(), Some("again"));
        }
        #[test]
        fn malformed_changed_store_errors_instead_of_serving_cached_auth_or_preferences() {
            let mut f = Fixture::new();
            f.replace("old");
            let _ = f.get("key").unwrap();
            std::fs::write(&f.path, b"invalid json").unwrap();
            assert_eq!(f.get("key").unwrap_err().kind(), io::ErrorKind::InvalidData);
            f.replace("repaired");
            assert_eq!(f.get("key").unwrap().as_deref(), Some("repaired"));
        }
        #[test]
        fn failed_write_is_not_cached_and_external_writer_values_are_merged() {
            let mut f = Fixture::new();
            f.replace("old");
            let _ = f.get("key").unwrap();
            let error = f.cache.update_with(
                &f.path,
                |map| {
                    map.insert("key".into(), "unwritten".into());
                    true
                },
                |_, _| {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "injected durable write failure",
                    ))
                },
            );
            assert_eq!(error.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
            assert_eq!(f.get("key").unwrap().as_deref(), Some("old"));
            f.replace("external");
            f.cache
                .update(&f.path, |map| {
                    map.insert("other".into(), "local".into());
                    true
                })
                .unwrap();
            assert_eq!(f.get("key").unwrap().as_deref(), Some("external"));
            assert_eq!(f.get("other").unwrap().as_deref(), Some("local"));
        }
        #[test]
        fn external_process_writes_merge_and_invalidate_parent_cache() {
            let mut f = Fixture::new();
            f.replace("parent");
            let _ = f.get("key").unwrap();
            let mut children: Vec<_> = (0..3)
                .map(|worker| {
                    std::process::Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            &format!(
                                "{}::isolated_cache_writer_child",
                                module_path!().split_once("::").unwrap().1
                            ),
                        ])
                        .env("GS26_CACHE_TEST_CHILD_PATH", &f.path)
                        .env("GS26_CACHE_TEST_CHILD_WORKER", worker.to_string())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::piped())
                        .spawn()
                        .unwrap()
                })
                .collect();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            for child in &mut children {
                loop {
                    if let Some(status) = child.try_wait().unwrap() {
                        assert!(status.success());
                        break;
                    }
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        panic!("isolated preference writer timed out");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            for worker in 0..3 {
                for index in 0..12 {
                    assert_eq!(
                        f.get(&format!("worker-{worker}-{index}"))
                            .unwrap()
                            .as_deref(),
                        Some("saved")
                    );
                }
            }
            assert_eq!(f.get("key").unwrap().as_deref(), Some("parent"));
        }
        #[test]
        fn isolated_cache_writer_child() {
            let Some(path) = std::env::var_os("GS26_CACHE_TEST_CHILD_PATH") else {
                return;
            };
            let path = std::path::PathBuf::from(path);
            let worker = std::env::var("GS26_CACHE_TEST_CHILD_WORKER").unwrap();
            let mut cache = CachedStore {
                path: None,
                identity: None,
                map: HashMap::new(),
                reloads: 0,
            };
            for index in 0..12 {
                cache
                    .update(&path, |map| {
                        map.insert(format!("worker-{worker}-{index}"), "saved".into());
                        true
                    })
                    .unwrap();
            }
        }
        #[test]
        fn no_op_update_does_not_replace_file_and_path_changes_do_not_share_values() {
            let mut f = Fixture::new();
            f.replace("old");
            let before = file_identity(&f.path).unwrap();
            f.cache.update(&f.path, |_| false).unwrap();
            assert!(file_identity(&f.path).unwrap() == before);
            let other = f.directory.join("other.json");
            assert_eq!(
                f.cache.read(&other, |map| map.get("key").cloned()).unwrap(),
                None
            );
            assert_eq!(f.get("key").unwrap().as_deref(), Some("old"));
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    mod tests {
        use super::*;
        use std::process::Command;

        #[test]
        fn native_restart_preserves_backend_and_remembered_session_until_signout() {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "gs26-native-restart-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(directory.join("first-build")).unwrap();
            std::fs::create_dir_all(directory.join("rebuilt-app")).unwrap();
            let run = |phase: &str, worker: Option<usize>| {
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--exact",
                        "telemetry_dashboard::persist::native::tests::native_restart_child",
                        "--nocapture",
                    ])
                    .env("GS26_PERSIST_TEST_PHASE", phase)
                    .env("XDG_DATA_HOME", directory.join("data"))
                    .current_dir(directory.join(if phase == "write" {
                        "first-build"
                    } else {
                        "rebuilt-app"
                    }))
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                if let Some(worker) = worker {
                    command.env("GS26_PERSIST_TEST_WORKER", worker.to_string());
                }
                command.spawn().unwrap()
            };
            let finish = |mut child: std::process::Child| {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while child.try_wait().unwrap().is_none() {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        panic!("isolated persistence child exceeded deadline");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                let result = child.wait_with_output().unwrap();
                assert!(
                    result.status.success(),
                    "{} {}",
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                );
                assert!(
                    String::from_utf8_lossy(&result.stdout)
                        .contains("NATIVE_PERSISTENCE_CHILD_PASS"),
                    "child test must actually execute"
                );
            };
            finish(run("write", None));
            let workers: Vec<_> = (0..4).map(|worker| run("merge", Some(worker))).collect();
            for worker in workers {
                finish(worker);
            }
            let mut holder = run("hold_lock", None);
            let ready = directory.join("data/lock-acquired");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !ready.exists() {
                if std::time::Instant::now() >= deadline {
                    let _ = holder.kill();
                    let _ = holder.wait();
                    panic!("child never acquired actual storage lock");
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            holder.kill().unwrap();
            holder.wait().unwrap();
            for phase in [
                "after_kill",
                "read_and_signout",
                "verify_signout",
                "ephemeral",
                "verify_signout",
                "malformed",
                "recover_empty",
            ] {
                finish(run(phase, None));
            }
            std::fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn native_restart_child() {
            let Ok(phase) = std::env::var("GS26_PERSIST_TEST_PHASE") else {
                return;
            };
            let backend = "https://native-storage.invalid:443";
            match phase.as_str() {
                "write" => {
                    set_string("gs_base_url", backend).unwrap();
                    crate::auth::init_from_storage(backend);
                    crate::auth::set_current_session(crate::auth::StoredAuthSession {
                        token: "native-persistence-test-only".to_owned(),
                        session: crate::auth::SessionStatus {
                            authenticated: true,
                            username: Some("test-only".to_owned()),
                            ..Default::default()
                        },
                        remember_me: true,
                    });
                    let workers: Vec<_> = (0..8)
                        .map(|worker| {
                            std::thread::spawn(move || {
                                for index in 0..16 {
                                    set_string(
                                        &format!("worker-{worker}-{index}"),
                                        &"x".repeat(2048),
                                    )
                                    .unwrap();
                                }
                            })
                        })
                        .collect();
                    for worker in workers {
                        worker.join().unwrap();
                    }
                    for worker in 0..8 {
                        for index in 0..16 {
                            assert_eq!(
                                get_string(&format!("worker-{worker}-{index}"))
                                    .unwrap()
                                    .unwrap()
                                    .len(),
                                2048
                            );
                        }
                    }
                    assert_eq!(get_string("gs_base_url").unwrap().as_deref(), Some(backend));
                }
                "merge" => {
                    let worker = std::env::var("GS26_PERSIST_TEST_WORKER").unwrap();
                    let peer = format!("https://process-{worker}.invalid");
                    crate::auth::init_from_storage(&peer);
                    crate::auth::set_current_session(crate::auth::StoredAuthSession {
                        token: format!("test-only-peer-{worker}"),
                        session: crate::auth::SessionStatus {
                            authenticated: true,
                            username: Some("test-only".to_owned()),
                            ..Default::default()
                        },
                        remember_me: true,
                    });
                    for index in 0..20 {
                        set_string(&format!("process-{worker}-{index}"), &"p".repeat(4096))
                            .unwrap();
                    }
                }
                "hold_lock" => {
                    let _lock = crate::native_storage::transaction_lock(&storage_path()).unwrap();
                    let root = std::path::PathBuf::from(std::env::var_os("XDG_DATA_HOME").unwrap());
                    std::fs::write(root.join("lock-acquired"), b"actual OS lock held").unwrap();
                    loop {
                        std::thread::park_timeout(std::time::Duration::from_secs(60));
                    }
                }
                "after_kill" => {
                    set_string("after-killed-writer", "preserved").unwrap();
                }
                "read_and_signout" => {
                    assert_eq!(
                        crate::telemetry_dashboard::UrlConfig::_stored_base_url().as_deref(),
                        Some(backend),
                        "actual connect UI must read saved backend"
                    );
                    assert_eq!(
                        crate::telemetry_dashboard::persisted_base_http_for_native_io(),
                        backend,
                        "native transports must use the same saved backend"
                    );
                    for worker in 0..4 {
                        crate::auth::init_from_storage(&format!(
                            "https://process-{worker}.invalid"
                        ));
                        assert_eq!(
                            crate::auth::current_token(),
                            Some(format!("test-only-peer-{worker}")),
                            "concurrent host-scoped sessions must merge"
                        );
                    }
                    assert_eq!(
                        get_string("after-killed-writer").unwrap().as_deref(),
                        Some("preserved")
                    );
                    for worker in 0..4 {
                        for index in 0..20 {
                            assert_eq!(
                                get_string(&format!("process-{worker}-{index}"))
                                    .unwrap()
                                    .unwrap()
                                    .len(),
                                4096
                            );
                        }
                    }

                    assert_eq!(get_string("gs_base_url").unwrap().as_deref(), Some(backend));
                    crate::auth::init_from_storage(backend);
                    assert_eq!(
                        crate::auth::current_token().as_deref(),
                        Some("native-persistence-test-only")
                    );
                    assert!(
                        crate::auth::current_status().command_proxy.is_none(),
                        "cached account must not restore live command authority"
                    );
                    crate::auth::init_from_storage("https://different-station.invalid");
                    assert!(
                        crate::auth::current_token().is_none(),
                        "never reuse token for another backend"
                    );
                    crate::auth::init_from_storage(backend);
                    assert!(crate::auth::current_token().is_some());
                    crate::auth::clear_current_session();
                    assert!(crate::auth::current_token().is_none());
                    assert_eq!(get_string("gs_base_url").unwrap().as_deref(), Some(backend));
                }
                "verify_signout" => {
                    crate::auth::init_from_storage(backend);
                    assert!(
                        crate::auth::current_token().is_none(),
                        "signout survives another process restart"
                    );
                    assert_eq!(get_string("gs_base_url").unwrap().as_deref(), Some(backend));
                }
                "ephemeral" => {
                    crate::auth::init_from_storage(backend);
                    crate::auth::set_current_session(crate::auth::StoredAuthSession {
                        token: "ephemeral-test-only".to_owned(),
                        session: Default::default(),
                        remember_me: false,
                    });
                    assert_eq!(
                        crate::auth::current_token().as_deref(),
                        Some("ephemeral-test-only")
                    );
                }
                "malformed" => {
                    std::fs::write(storage_path(), b"{broken nonempty preferences").unwrap();
                    assert_eq!(
                        set_string("gs_base_url", backend).unwrap_err().kind(),
                        io::ErrorKind::InvalidData
                    );
                    assert_eq!(
                        std::fs::read(storage_path()).unwrap(),
                        b"{broken nonempty preferences"
                    );
                }
                "recover_empty" => {
                    std::fs::write(storage_path(), b"").unwrap();
                    set_string("gs_base_url", backend).unwrap();
                    assert_eq!(get_string("gs_base_url").unwrap().as_deref(), Some(backend));
                }
                _ => panic!("unknown isolated test phase"),
            }
            println!("NATIVE_PERSISTENCE_CHILD_PASS");
        }
    }
}

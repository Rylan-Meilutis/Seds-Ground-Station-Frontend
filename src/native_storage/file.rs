//! Crash-safe replacement of native settings/session files. No values are logged.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Lock a stable sidecar, not the data inode that atomic replacement renames.
/// Closing the returned handle releases the OS lock, including process death.
pub(crate) fn transaction_lock(path: &Path) -> io::Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "storage path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "storage path has no filename"))?
        .to_os_string();
    name.push(".lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(parent.join(name))?;
    fs4::fs_std::FileExt::lock_exclusive(&file)?;
    Ok(file)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace_with(path, |file| file.write_all(bytes))
}

fn replace_with(path: &Path, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "storage path has no parent"))?;
    fs::create_dir_all(parent)?;
    let (temporary, mut file) = loop {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".gs26-{}-{id}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => break (Temporary(candidate), file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    write(&mut file)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary.0, path)?;
    sync_parent(parent)
}

fn sync_parent(parent: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
    }
    Ok(())
}

pub(crate) fn remove_durable(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "storage path has no parent")
        })?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "gs26-storage-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn advisory_lock_releases_with_handle_and_does_not_lock_replaced_inode() {
        let dir = directory();
        let path = dir.join("storage.json");
        let holder = transaction_lock(&path).unwrap();
        atomic_write(&path, b"complete").unwrap();
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join("storage.json.lock"))
            .unwrap();
        assert!(!fs4::fs_std::FileExt::try_lock_exclusive(&competing).unwrap());
        atomic_write(&path, b"replacement").unwrap();
        assert!(
            !fs4::fs_std::FileExt::try_lock_exclusive(&competing).unwrap(),
            "rename must not invalidate sidecar lock"
        );
        drop(holder);
        assert!(fs4::fs_std::FileExt::try_lock_exclusive(&competing).unwrap());
        drop(competing);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interrupted_replacement_preserves_previous_file_and_removes_temporary() {
        let dir = directory();
        let path = dir.join("auth_session.json");
        atomic_write(&path, b"previous complete session").unwrap();
        let result = replace_with(&path, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("simulated interrupted writer"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"previous complete session");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        atomic_write(&path, b"next complete session").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"next complete session");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        remove_durable(&path).unwrap();
        remove_durable(&path).unwrap();
        assert!(!path.exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn readers_only_observe_complete_versions_during_replacement() {
        let dir = directory();
        let path = dir.join("storage.json");
        let a = vec![b'a'; 65536];
        let b = vec![b'b'; 65536];
        atomic_write(&path, &a).unwrap();
        let read_path = path.clone();
        let reader = std::thread::spawn(move || {
            for _ in 0..200 {
                let data = fs::read(&read_path).unwrap();
                assert_eq!(data.len(), 65536);
                assert!(data.iter().all(|v| *v == data[0]));
            }
        });
        for i in 0..40 {
            atomic_write(&path, if i % 2 == 0 { &b } else { &a }).unwrap();
        }
        reader.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}

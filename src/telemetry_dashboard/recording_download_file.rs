//! One call owns one partial; cancellation never touches another export.
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_EXPORT: AtomicU64 = AtomicU64::new(1);

pub(super) struct Output {
    file: Option<File>,
    partial: PathBuf,
    target: PathBuf,
}
impl Output {
    pub(super) fn create(directory: &Path, filename: &str) -> io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        let mut id = NEXT_EXPORT.load(Ordering::SeqCst);
        loop {
            let next = id
                .checked_add(1)
                .ok_or_else(|| io::Error::other("Export identity exhausted"))?;
            match NEXT_EXPORT.compare_exchange(id, next, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(current) => id = current,
            }
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let name = Path::new(filename);
        let stem = name
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("telemetry");
        let extension = name.extension().and_then(|s| s.to_str()).unwrap_or("csv");
        let target = directory.join(format!("{stem}-{stamp}-{id}.{extension}"));
        let partial = directory.join(format!(
            ".gs-export-{}-{stamp}-{id}.part",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&partial)?;
        Ok(Self {
            file: Some(file),
            partial,
            target,
        })
    }
    pub(super) fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("Export is closed"))?
            .write_all(bytes)
    }
    pub(super) fn finish(
        mut self,
        authorize: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<PathBuf> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("Export is closed"))?
            .flush()?;
        // Close first: Windows cannot unlink our live file on cancellation.
        drop(self.file.take());
        // Same directory/volume; unlike rename this never replaces a user file.
        // Unsupported filesystems fail without publishing or overwriting data.
        authorize()?;
        std::fs::hard_link(&self.partial, &self.target)?;
        Ok(self.target.clone())
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.partial);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> PathBuf {
        let n = NEXT_EXPORT.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("gs-export-output-{}-{n}", std::process::id()))
    }
    #[test]
    fn cancellation_removes_only_this_owned_partial() {
        let dir = directory();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("preexisting.csv"), b"keep").unwrap();
        let mut first = Output::create(&dir, "same.csv").unwrap();
        first.write(b"private partial").unwrap();
        let mut second = Output::create(&dir, "same.csv").unwrap();
        second.write(b"other ticket").unwrap();
        let other = second.partial.clone();
        let own = first.partial.clone();
        drop(first);
        assert!(!own.exists());
        assert!(other.exists());
        let committed = second.finish(|| Ok(())).unwrap();
        assert_eq!(std::fs::read(committed).unwrap(), b"other ticket");
        assert_eq!(std::fs::read(dir.join("preexisting.csv")).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn publication_never_overwrites_an_existing_target() {
        let dir = directory();
        let mut output = Output::create(&dir, "test.csv").unwrap();
        output.write(b"new").unwrap();
        let target = output.target.clone();
        let partial = output.partial.clone();
        std::fs::write(&target, b"existing user file").unwrap();
        assert_eq!(
            output.finish(|| Ok(())).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(target).unwrap(), b"existing user file");
        assert!(!partial.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn revoked_after_close_never_publishes_or_deletes_another_file() {
        let dir = directory();
        let mut output = Output::create(&dir, "private.csv").unwrap();
        output.write(b"private").unwrap();
        let partial = output.partial.clone();
        let target = output.target.clone();
        let sentinel = dir.join("existing.csv");
        std::fs::write(&sentinel, b"keep").unwrap();
        assert_eq!(
            output
                .finish(|| {
                    assert!(partial.exists());
                    Err(io::Error::new(io::ErrorKind::PermissionDenied, "retired"))
                })
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(!partial.exists());
        assert!(!target.exists());
        assert_eq!(std::fs::read(sentinel).unwrap(), b"keep");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn output_is_private_and_complete() {
        let dir = directory();
        let mut output = Output::create(&dir, "recording.json").unwrap();
        output.write(b"first").unwrap();
        output.write(b"second").unwrap();
        let target = output.finish(|| Ok(())).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"firstsecond");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

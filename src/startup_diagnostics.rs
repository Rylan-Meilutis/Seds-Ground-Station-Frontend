//! iOS-only, payload-free diagnostics retrievable through Files after a failed launch.
//! Deliberately does not handle native signals or Objective-C exceptions.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    panic::Location,
    path::Path,
    sync::{Mutex, OnceLock},
};

const LIMIT: u64 = 64 * 1024;
static REPORT: OnceLock<Mutex<Report>> = OnceLock::new();

#[derive(Clone, Copy)]
pub(crate) enum Stage {
    PanicHookInstalled,
    CryptoReady,
    ConfigCreated,
    Launching,
    AppRendered,
    LaunchReturned,
}
impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::PanicHookInstalled => "panic_hook_installed",
            Self::CryptoReady => "crypto_ready",
            Self::ConfigCreated => "config_created",
            Self::Launching => "launching",
            Self::AppRendered => "app_rendered",
            Self::LaunchReturned => "launch_returned",
        }
    }
    fn bit(self) -> u8 {
        1 << self as u8
    }
}

struct Report {
    file: File,
    bytes: u64,
    stages: u8,
}
impl Report {
    fn open(directory: &Path) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let current = directory.join("launch-current.txt");
        if current.exists() {
            fs::rename(&current, directory.join("launch-previous.txt"))?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(current)?;
        let mut report = Self {
            file,
            bytes: 0,
            stages: 0,
        };
        report.append("GS26 launch diagnostics v1\nrust_main_entered\n")?;
        Ok(report)
    }
    fn append(&mut self, text: &str) -> io::Result<()> {
        if text.len() as u64 > LIMIT.saturating_sub(self.bytes) {
            return Ok(());
        }
        self.file.write_all(text.as_bytes())?;
        self.bytes += text.len() as u64;
        self.file.sync_data()
    }
    fn mark(&mut self, stage: Stage) -> io::Result<()> {
        if self.stages & stage.bit() == 0 {
            self.stages |= stage.bit();
            self.append(stage.name())?;
            self.append("\n")?;
        }
        Ok(())
    }
}

pub(crate) fn begin() {
    // HOME is the iOS application sandbox. Never use cwd or Library as fallback.
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let directory = Path::new(&home).join("Documents/GS26Diagnostics");
    if let Ok(report) = Report::open(&directory) {
        let _ = REPORT.set(Mutex::new(report));
    }
}

pub(crate) fn mark(stage: Stage) {
    if let Some(report) = REPORT.get() {
        if let Ok(mut report) = report.try_lock() {
            let _ = report.mark(stage);
        }
    }
}

pub(crate) fn panic_location(location: Option<&Location<'_>>) {
    if let Some(report) = REPORT.get() {
        if let Ok(mut report) = report.try_lock() {
            // No panic payload, request URL, user data, or environment values.
            let text = match location {
                Some(location) => format!(
                    "rust_panic line={} column={}\n",
                    location.line(),
                    location.column()
                ),
                None => "rust_panic location_unavailable\n".to_owned(),
            };
            let _ = report.append(&text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "gs-ios-diagnostic-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }
    #[test]
    fn durable_rotation_stage_dedup_and_limit() {
        let dir = directory();
        let mut first = Report::open(&dir).unwrap();
        for stage in [
            Stage::PanicHookInstalled,
            Stage::CryptoReady,
            Stage::ConfigCreated,
            Stage::Launching,
            Stage::AppRendered,
            Stage::LaunchReturned,
        ] {
            first.mark(stage).unwrap();
            first.mark(stage).unwrap();
        }
        first.append(&"x".repeat(LIMIT as usize)).unwrap();
        drop(first);
        let before = fs::read_to_string(dir.join("launch-current.txt")).unwrap();
        assert_eq!(before.matches("app_rendered").count(), 1);
        assert!(before.len() < LIMIT as usize);
        drop(Report::open(&dir).unwrap());
        assert_eq!(
            fs::read_to_string(dir.join("launch-previous.txt")).unwrap(),
            before
        );
        assert_eq!(
            fs::read_to_string(dir.join("launch-current.txt")).unwrap(),
            "GS26 launch diagnostics v1\nrust_main_entered\n"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn storage_failure_does_not_require_private_library_fallback() {
        let dir = directory();
        fs::write(&dir, "not a directory").unwrap();
        assert!(Report::open(&dir).is_err());
        fs::remove_file(dir).unwrap();
        // Public entry points safely tolerate no initialized report.
        mark(Stage::Launching);
        panic_location(None);
        // Exercise absent HOME without mutating the process environment.
        let _begin: fn() = begin;
    }
}

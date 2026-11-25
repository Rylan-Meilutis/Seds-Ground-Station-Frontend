//! Replay files opened recently on this desktop, listed on the connection
//! page so one can be opened again with one click (docs/offline-preview.md,
//! "Opening a replay file").
//!
//! Only files the app can open again by path are kept: a file picked or
//! dropped on a desktop, or opened from Finder, a file manager or the command
//! line. Phones hand over copies or security-scoped URLs that do not outlive
//! the session, and the browser never sees a path, so neither keeps a list.
//! The list holds names and paths only, in its own small file beside the
//! app's settings; it is read on the connection page and written when a
//! replay opens, never while a station is connected.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Entries kept, newest first.
pub(crate) const MAX_RECENT: usize = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct RecentReplay {
    pub name: String,
    pub path: PathBuf,
}

fn store_path() -> PathBuf {
    crate::native_storage::fallback_data_dir().join("gs26").join("recent_replays.json")
}

/// `list` with `opened` moved to the front, without duplicates, at most
/// [`MAX_RECENT`].
fn with_opened(mut list: Vec<RecentReplay>, opened: RecentReplay) -> Vec<RecentReplay> {
    list.retain(|entry| entry.path != opened.path);
    list.insert(0, opened);
    list.truncate(MAX_RECENT);
    list
}

fn read_from(path: &Path) -> Vec<RecentReplay> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<RecentReplay>>(&bytes).ok())
        .unwrap_or_default()
}

fn write_to(path: &Path, list: &[RecentReplay]) {
    let Ok(bytes) = serde_json::to_vec(list) else { return };
    if let Err(error) = crate::native_storage::atomic_write(path, &bytes) {
        crate::native_storage::report_error("recent replays write", &error);
    }
}

fn record_at(store: &Path, name: &str, path: &Path) {
    let opened = RecentReplay { name: name.to_string(), path: path.to_path_buf() };
    write_to(store, &with_opened(read_from(store), opened));
}

fn list_at(store: &Path) -> Vec<RecentReplay> {
    read_from(store).into_iter().filter(|entry| entry.path.is_file()).take(MAX_RECENT).collect()
}

fn forget_at(store: &Path, path: &Path) {
    let mut list = read_from(store);
    let before = list.len();
    list.retain(|entry| entry.path != path);
    if list.len() != before {
        write_to(store, &list);
    }
}

/// Remembers a replay that opened from `path`.
pub(crate) fn record(name: &str, path: &Path) {
    record_at(&store_path(), name, path);
}

/// The remembered replays whose files still exist, newest first.
pub(crate) fn list() -> Vec<RecentReplay> {
    list_at(&store_path())
}

/// Drops a remembered replay (it could not be opened again).
pub(crate) fn forget(path: &Path) {
    forget_at(&store_path(), path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> RecentReplay {
        RecentReplay { name: name.into(), path: PathBuf::from(format!("/replays/{name}")) }
    }

    #[test]
    fn newest_first_without_duplicates_and_bounded() {
        let mut list = Vec::new();
        for name in ["a", "b", "c", "d", "e", "f"] {
            list = with_opened(list, entry(name));
        }
        assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["f", "e", "d", "c", "b"]);
        let list = with_opened(list, entry("d"));
        assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["d", "f", "e", "c", "b"]);
    }

    #[test]
    fn stored_list_skips_missing_files_and_forgets() {
        let dir = std::env::temp_dir().join(format!("gs26-recent-replays-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = dir.join("recent_replays.json");
        let kept = dir.join("kept.gsreplay");
        let gone = dir.join("gone.gsreplay");
        std::fs::write(&kept, b"PK").unwrap();
        std::fs::write(&gone, b"PK").unwrap();
        assert!(list_at(&store).is_empty(), "no store yet: empty list");
        record_at(&store, "kept.gsreplay", &kept);
        record_at(&store, "gone.gsreplay", &gone);
        assert_eq!(list_at(&store).len(), 2);
        std::fs::remove_file(&gone).unwrap();
        assert_eq!(list_at(&store), vec![RecentReplay { name: "kept.gsreplay".into(), path: kept.clone() }]);
        forget_at(&store, &kept);
        assert!(list_at(&store).is_empty());
        std::fs::write(&store, b"not json").unwrap();
        assert!(list_at(&store).is_empty(), "a damaged list reads as empty");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Recoverable local JSON storage in a caller-provided directory.
//!
//! Files in the directory:
//! * `focus-return.json`       — current data.
//! * `focus-return.json.bak`   — the previous valid data (one save behind).
//! * `focus-return.json.tmp`, `focus-return.json.bak.tmp` — scratch files for atomic replace.
//! * `focus-return.<primary|backup>.corrupt-<ms>[-n].json` — byte-exact copies of damaged files.
//!
//! Guarantees:
//! * A save writes and flushes a temp file, then replaces the target with a rename
//!   (on Windows `std::fs::rename` replaces an existing file). The existing data file
//!   is never truncated or deleted first, so a failed save leaves it intact.
//! * No state survives a failed save: the next save simply runs again.
//! * Damaged data is copied byte-for-byte (`create_new`, never overwriting) before
//!   anything can replace it. Unreadable files (I/O errors such as a lock) are *not*
//!   treated as damaged; the error is returned and nothing is changed.
//! * A file from a newer schema version is never overwritten.
//! * Nothing is logged or sent anywhere; errors carry paths and categories, not content.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::StorageError;
use crate::model::{AppData, SCHEMA_VERSION};
use crate::EpochMs;

pub const DATA_FILE_NAME: &str = "focus-return.json";
pub const BACKUP_FILE_NAME: &str = "focus-return.json.bak";
const TEMP_FILE_NAME: &str = "focus-return.json.tmp";
const BACKUP_TEMP_FILE_NAME: &str = "focus-return.json.bak.tmp";
const PRESERVE_ATTEMPTS: u32 = 1000;

/// Where the loaded data came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadSource {
    /// No data file yet (first launch).
    Fresh,
    Primary,
    /// The main file was missing or damaged; the previous valid version was used.
    Backup,
    /// Main and backup were damaged or missing after damage; defaults were used.
    DefaultsAfterCorruption,
}

/// Why a file was considered damaged. Contains no file content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorruptionKind {
    NotJson,
    MissingSchemaVersion,
    UnsupportedOldVersion,
    SchemaMismatch,
    InvalidData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreservedFile {
    pub path: PathBuf,
    pub kind: CorruptionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadReport {
    pub data: AppData,
    pub source: LoadSource,
    pub preserved: Vec<PreservedFile>,
}

impl LoadReport {
    /// True when the UI should tell the user that data was recovered or reset.
    pub fn needs_user_notice(&self) -> bool {
        !matches!(self.source, LoadSource::Fresh | LoadSource::Primary) || !self.preserved.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct SaveReport {
    /// A damaged main file found during save, preserved before being replaced.
    pub preserved: Vec<PreservedFile>,
    /// The backup could not be refreshed. The save itself still succeeded.
    pub backup_error: Option<StorageError>,
}

enum Inspection {
    Missing,
    Valid { data: AppData, bytes: Vec<u8> },
    Newer { found: u64 },
    Corrupt { bytes: Vec<u8>, kind: CorruptionKind },
}

/// Storage rooted at a directory chosen by the caller (e.g. Tauri's app data dir).
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn data_path(&self) -> PathBuf {
        self.dir.join(DATA_FILE_NAME)
    }

    pub fn backup_path(&self) -> PathBuf {
        self.dir.join(BACKUP_FILE_NAME)
    }

    fn temp_path(&self) -> PathBuf {
        self.dir.join(TEMP_FILE_NAME)
    }

    fn backup_temp_path(&self) -> PathBuf {
        self.dir.join(BACKUP_TEMP_FILE_NAME)
    }

    /// Loads data, falling back to the backup and then to defaults. Damaged files
    /// are preserved first. `now` only names the preserved copies.
    ///
    /// Returns an error (and changes nothing) when a file cannot be read or was
    /// written by a newer version; the caller should show it and offer a retry.
    pub fn load(&self, now: EpochMs) -> Result<LoadReport, StorageError> {
        let primary = self.data_path();
        let mut preserved = Vec::new();
        match inspect(&primary)? {
            Inspection::Valid { data, .. } => {
                return Ok(LoadReport {
                    data,
                    source: LoadSource::Primary,
                    preserved,
                })
            }
            Inspection::Newer { found } => return Err(newer_schema(primary, found)),
            Inspection::Missing => {}
            Inspection::Corrupt { bytes, kind } => {
                preserved.push(self.quarantine(&primary, &bytes, "primary", kind, now)?);
            }
        }

        let backup = self.backup_path();
        let (data, source) = match inspect(&backup)? {
            Inspection::Valid { data, .. } => (data, LoadSource::Backup),
            Inspection::Newer { found } => return Err(newer_schema(backup, found)),
            Inspection::Missing if preserved.is_empty() => (AppData::default(), LoadSource::Fresh),
            Inspection::Missing => (AppData::default(), LoadSource::DefaultsAfterCorruption),
            Inspection::Corrupt { bytes, kind } => {
                preserved.push(self.quarantine(&backup, &bytes, "backup", kind, now)?);
                (AppData::default(), LoadSource::DefaultsAfterCorruption)
            }
        };
        Ok(LoadReport {
            data,
            source,
            preserved,
        })
    }

    /// Validates and atomically saves `data`. The previous valid file becomes the backup.
    pub fn save(&self, data: &AppData, now: EpochMs) -> Result<SaveReport, StorageError> {
        data.validate().map_err(StorageError::InvalidData)?;
        let json = serde_json::to_vec_pretty(data).map_err(StorageError::Serialize)?;
        fs::create_dir_all(&self.dir)
            .map_err(|source| io_error("create data directory", &self.dir, source))?;

        let primary = self.data_path();
        let mut report = SaveReport::default();
        match inspect(&primary)? {
            Inspection::Missing => {}
            Inspection::Newer { found } => return Err(newer_schema(primary, found)),
            Inspection::Corrupt { bytes, kind } => {
                let path = self.preserve(&bytes, "primary", now)?;
                report.preserved.push(PreservedFile { path, kind });
            }
            Inspection::Valid { bytes, .. } => {
                // A failing backup must not block saving the user's latest data.
                if let Err(err) = write_atomic(&self.backup_temp_path(), &self.backup_path(), &bytes) {
                    report.backup_error = Some(err);
                }
            }
        }

        write_atomic(&self.temp_path(), &primary, &json)?;
        Ok(report)
    }

    /// Preserves a damaged file, then removes the original so later saves start clean.
    /// If removal fails the original simply stays; the preserved copy is already durable.
    fn quarantine(
        &self,
        source: &Path,
        bytes: &[u8],
        label: &str,
        kind: CorruptionKind,
        now: EpochMs,
    ) -> Result<PreservedFile, StorageError> {
        let path = self.preserve(bytes, label, now)?;
        let _ = fs::remove_file(source);
        Ok(PreservedFile { path, kind })
    }

    /// Writes a byte-exact copy under a name that does not exist yet.
    fn preserve(&self, bytes: &[u8], label: &str, now: EpochMs) -> Result<PathBuf, StorageError> {
        for attempt in 0..PRESERVE_ATTEMPTS {
            let name = if attempt == 0 {
                format!("focus-return.{label}.corrupt-{now}.json")
            } else {
                format!("focus-return.{label}.corrupt-{now}-{attempt}.json")
            };
            let path = self.dir.join(name);
            let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => file,
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(io_error("create preserved copy", &path, err)),
            };
            file.write_all(bytes)
                .map_err(|err| io_error("write preserved copy", &path, err))?;
            file.sync_all()
                .map_err(|err| io_error("flush preserved copy", &path, err))?;
            return Ok(path);
        }
        Err(io_error(
            "create preserved copy",
            &self.dir,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "too many preserved copies with the same timestamp",
            ),
        ))
    }
}

fn inspect(path: &Path) -> Result<Inspection, StorageError> {
    match fs::read(path) {
        Ok(bytes) => Ok(classify(bytes)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Inspection::Missing),
        Err(err) => Err(io_error("read data file", path, err)),
    }
}

fn classify(bytes: Vec<u8>) -> Inspection {
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return Inspection::Corrupt {
                bytes,
                kind: CorruptionKind::NotJson,
            }
        }
    };
    let corrupt_kind = match value.get("schemaVersion").and_then(Value::as_u64) {
        None => Some(CorruptionKind::MissingSchemaVersion),
        Some(found) if found > u64::from(SCHEMA_VERSION) => return Inspection::Newer { found },
        // Migrations from older versions would go here; version 1 is the first.
        Some(found) if found < u64::from(SCHEMA_VERSION) => Some(CorruptionKind::UnsupportedOldVersion),
        Some(_) => None,
    };
    if let Some(kind) = corrupt_kind {
        return Inspection::Corrupt { bytes, kind };
    }
    let data: AppData = match serde_json::from_value(value) {
        Ok(data) => data,
        Err(_) => {
            return Inspection::Corrupt {
                bytes,
                kind: CorruptionKind::SchemaMismatch,
            }
        }
    };
    if data.validate().is_err() {
        return Inspection::Corrupt {
            bytes,
            kind: CorruptionKind::InvalidData,
        };
    }
    Inspection::Valid { data, bytes }
}

/// Temp file + flush + rename. The target is only touched by the final rename.
/// A leftover temp file from a failed attempt is truncated by the next attempt.
fn write_atomic(temp: &Path, target: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let mut file = File::create(temp).map_err(|err| io_error("create temp file", temp, err))?;
    file.write_all(bytes)
        .map_err(|err| io_error("write temp file", temp, err))?;
    file.sync_all()
        .map_err(|err| io_error("flush temp file", temp, err))?;
    // The handle must be closed before renaming on Windows.
    drop(file);
    fs::rename(temp, target).map_err(|err| io_error("replace file", target, err))?;
    sync_parent_dir(target);
    Ok(())
}

/// Makes the rename durable on Unix. Windows has no portable directory flush in std.
#[cfg(unix)]
fn sync_parent_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
}

#[cfg(not(unix))]
fn sync_parent_dir(_path: &Path) {}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> StorageError {
    StorageError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn newer_schema(path: PathBuf, found: u64) -> StorageError {
    StorageError::NewerSchema {
        path,
        found,
        supported: SCHEMA_VERSION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SessionOutcome, SessionRecord, TaskName};
    use crate::timer::{RunId, SessionKind};
    use std::sync::atomic::{AtomicU64, Ordering};

    const NOW: EpochMs = 1_800_000_000_000;

    /// A unique directory under the OS temp dir, removed on drop. Tests never touch
    /// any other path.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "focus-return-core-test-{}-{}-{name}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn store(&self) -> Store {
            Store::new(&self.0)
        }

        fn files(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn data_with_task(name: &str) -> AppData {
        let mut data = AppData::default();
        data.draft.task = TaskName::parse_optional(name).unwrap();
        data
    }

    fn data_with_session() -> AppData {
        let mut data = AppData::default();
        data.record_session(SessionRecord {
            id: RunId(1),
            kind: SessionKind::Focus,
            task: TaskName::untitled(),
            next_action: None,
            started_at: NOW,
            ended_at: NOW + 600_000,
            duration_ms: 600_000,
            timer_run_ms: 600_000,
            outcome: SessionOutcome::Completed,
            return_count: 1,
        })
        .unwrap();
        data
    }

    fn json_with(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
        let mut value = serde_json::to_value(AppData::default()).unwrap();
        edit(&mut value);
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn first_load_returns_defaults_without_writing() {
        let dir = TempDir::new("fresh");
        let report = dir.store().load(NOW).unwrap();
        assert_eq!(report.source, LoadSource::Fresh);
        assert_eq!(report.data, AppData::default());
        assert!(!report.needs_user_notice());
        assert!(dir.files().is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new("roundtrip");
        let store = dir.store();
        let data = data_with_session();
        let report = store.save(&data, NOW).unwrap();
        assert!(report.preserved.is_empty() && report.backup_error.is_none());

        let loaded = store.load(NOW).unwrap();
        assert_eq!(loaded.source, LoadSource::Primary);
        assert_eq!(loaded.data, data);
        assert_eq!(dir.files(), [DATA_FILE_NAME]);
    }

    #[test]
    fn save_creates_missing_directory() {
        let dir = TempDir::new("nested");
        let store = Store::new(dir.0.join("a").join("b"));
        let _ = store.save(&AppData::default(), NOW).unwrap();
        assert!(store.data_path().is_file());
    }

    #[test]
    fn save_replaces_existing_file_and_keeps_previous_as_backup() {
        let dir = TempDir::new("backup");
        let store = dir.store();
        let first = data_with_task("첫 번째");
        let second = data_with_task("두 번째");
        let _ = store.save(&first, NOW).unwrap();
        let _ = store.save(&second, NOW + 1).unwrap();

        assert_eq!(store.load(NOW).unwrap().data, second);
        let backup: AppData = serde_json::from_slice(&fs::read(store.backup_path()).unwrap()).unwrap();
        assert_eq!(backup, first);
    }

    #[test]
    fn failed_save_keeps_original_and_retry_succeeds() {
        let dir = TempDir::new("retry");
        let store = dir.store();
        let original = data_with_task("원래 작업");
        let _ = store.save(&original, NOW).unwrap();
        let original_bytes = fs::read(store.data_path()).unwrap();

        // Make the temp path unusable so the next save fails before any rename.
        fs::create_dir(store.temp_path()).unwrap();
        let updated = data_with_task("바뀐 작업");
        let err = store.save(&updated, NOW + 1).unwrap_err();
        assert!(matches!(err, StorageError::Io { .. }), "{err}");
        assert_eq!(fs::read(store.data_path()).unwrap(), original_bytes);
        assert_eq!(store.load(NOW + 2).unwrap().data, original);

        // Once the obstacle is gone, the very next save works; nothing stayed "stuck".
        fs::remove_dir(store.temp_path()).unwrap();
        let _ = store.save(&updated, NOW + 3).unwrap();
        assert_eq!(store.load(NOW + 4).unwrap().data, updated);
    }

    #[test]
    fn invalid_data_is_rejected_before_touching_disk() {
        let dir = TempDir::new("invalid-save");
        let store = dir.store();
        let mut data = data_with_session();
        data.last_run_id = 0;
        assert!(matches!(store.save(&data, NOW), Err(StorageError::InvalidData(_))));
        assert!(dir.files().is_empty());
    }

    #[test]
    fn corrupt_main_file_is_preserved_and_backup_is_used() {
        let dir = TempDir::new("corrupt-primary");
        let store = dir.store();
        let good = data_with_task("백업될 작업");
        let _ = store.save(&good, NOW).unwrap();
        let _ = store.save(&data_with_task("최신 작업"), NOW + 1).unwrap();
        let garbage = b"{\"schemaVersion\": 1, \"settings\": {".to_vec();
        fs::write(store.data_path(), &garbage).unwrap();

        let report = store.load(NOW + 2).unwrap();
        assert_eq!(report.source, LoadSource::Backup);
        assert_eq!(report.data, good);
        assert!(report.needs_user_notice());
        assert_eq!(report.preserved.len(), 1);
        assert_eq!(report.preserved[0].kind, CorruptionKind::NotJson);
        assert_eq!(fs::read(&report.preserved[0].path).unwrap(), garbage);
        assert!(!store.data_path().exists());

        // Saving after recovery works and does not overwrite the good backup with garbage.
        let _ = store.save(&report.data, NOW + 3).unwrap();
        assert_eq!(store.load(NOW + 4).unwrap().source, LoadSource::Primary);
        assert_eq!(fs::read(&report.preserved[0].path).unwrap(), garbage);
    }

    #[test]
    fn corrupt_files_without_valid_backup_fall_back_to_defaults() {
        let dir = TempDir::new("corrupt-both");
        let store = dir.store();
        fs::write(store.data_path(), b"not json").unwrap();
        fs::write(store.backup_path(), b"").unwrap();

        let report = store.load(NOW).unwrap();
        assert_eq!(report.source, LoadSource::DefaultsAfterCorruption);
        assert_eq!(report.data, AppData::default());
        assert_eq!(report.preserved.len(), 2);
        assert_eq!(fs::read(&report.preserved[0].path).unwrap(), b"not json");
        assert_eq!(fs::read(&report.preserved[1].path).unwrap(), b"");
    }

    #[test]
    fn well_formed_json_with_invalid_values_is_treated_as_damaged() {
        let dir = TempDir::new("invalid-values");
        let store = dir.store();
        let bytes = json_with(|value| value["settings"]["defaultMinutes"] = 500.into());
        fs::write(store.data_path(), &bytes).unwrap();

        let report = store.load(NOW).unwrap();
        assert_eq!(report.source, LoadSource::DefaultsAfterCorruption);
        assert_eq!(report.preserved[0].kind, CorruptionKind::SchemaMismatch);
        assert_eq!(fs::read(&report.preserved[0].path).unwrap(), bytes);

        let missing_version = json_with(|value| {
            value.as_object_mut().unwrap().remove("schemaVersion");
        });
        fs::write(store.data_path(), &missing_version).unwrap();
        let report = store.load(NOW).unwrap();
        assert_eq!(report.preserved[0].kind, CorruptionKind::MissingSchemaVersion);
    }

    #[test]
    fn newer_schema_is_never_overwritten() {
        let dir = TempDir::new("newer");
        let store = dir.store();
        let bytes = json_with(|value| value["schemaVersion"] = 99.into());
        fs::write(store.data_path(), &bytes).unwrap();

        assert!(matches!(
            store.load(NOW),
            Err(StorageError::NewerSchema { found: 99, .. })
        ));
        assert!(matches!(
            store.save(&AppData::default(), NOW),
            Err(StorageError::NewerSchema { found: 99, .. })
        ));
        assert_eq!(fs::read(store.data_path()).unwrap(), bytes);
        assert_eq!(dir.files(), [DATA_FILE_NAME]);
    }

    #[test]
    fn save_preserves_a_main_file_damaged_after_load() {
        let dir = TempDir::new("damaged-before-save");
        let store = dir.store();
        fs::write(store.data_path(), b"\x00\x01 broken").unwrap();

        let report = store.save(&data_with_task("새 작업"), NOW).unwrap();
        assert_eq!(report.preserved.len(), 1);
        assert_eq!(fs::read(&report.preserved[0].path).unwrap(), b"\x00\x01 broken");
        assert!(!store.backup_path().exists(), "damaged data must not become the backup");
        assert_eq!(store.load(NOW).unwrap().source, LoadSource::Primary);
    }

    #[test]
    fn preserved_copies_never_overwrite_each_other() {
        let dir = TempDir::new("preserve-unique");
        let store = dir.store();
        for content in [&b"first"[..], &b"second"[..]] {
            fs::write(store.data_path(), content).unwrap();
            let _ = store.load(NOW).unwrap();
        }
        let preserved: Vec<String> = dir
            .files()
            .into_iter()
            .filter(|name| name.contains(".corrupt-"))
            .collect();
        assert_eq!(preserved.len(), 2);
        let contents: Vec<Vec<u8>> = preserved
            .iter()
            .map(|name| fs::read(dir.0.join(name)).unwrap())
            .collect();
        assert!(contents.contains(&b"first".to_vec()));
        assert!(contents.contains(&b"second".to_vec()));
    }

    #[test]
    fn unreadable_main_file_is_an_error_not_corruption() {
        let dir = TempDir::new("unreadable");
        let store = dir.store();
        // A directory in place of the data file cannot be read as a file.
        fs::create_dir(store.data_path()).unwrap();
        assert!(matches!(store.load(NOW), Err(StorageError::Io { .. })));
        assert!(store.data_path().is_dir());
        assert!(!dir.files().iter().any(|name| name.contains(".corrupt-")));
    }
}

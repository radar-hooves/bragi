//! The bundle identifier names Bragi's data, config and cache directories, so
//! changing it strands them. [`migrate_legacy_dirs`] copies each directory the
//! previous identifier owned to the new one, once, before anything opens them.

use std::path::{Path, PathBuf};

/// Must equal `identifier` in `tauri.conf.json` (checked by a test below).
pub const APP_ID: &str = "com.poodle64.bragi";

const LEGACY_APP_ID: &str = "au.com.pdr.bragi";

/// Copy, never move: the previously installed release keeps working if the
/// user goes back to it.
fn should_copy(old_exists: bool, new_exists: bool) -> bool {
    old_exists && !new_exists
}

/// The (old, new) directory pairs Tauri derives from the identifier:
/// `app_data_dir`, `app_config_dir` (window state; the same directory as data
/// on macOS and Windows) and `app_cache_dir` (artwork).
fn pairs() -> Vec<(PathBuf, PathBuf)> {
    let mut pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
    for base in [dirs::data_dir(), dirs::config_dir(), dirs::cache_dir()]
        .into_iter()
        .flatten()
    {
        let pair = (base.join(LEGACY_APP_ID), base.join(APP_ID));
        if !pairs.contains(&pair) {
            pairs.push(pair);
        }
    }
    pairs
}

/// Copies into a sibling `.migrating` directory and renames it into place, so
/// an interrupted copy never leaves a half-filled directory that the next
/// launch would take as the migrated one.
fn copy_dir_atomic(old: &Path, new: &Path) -> std::io::Result<()> {
    let mut staging = new.as_os_str().to_owned();
    staging.push(".migrating");
    let staging = PathBuf::from(staging);
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    copy_dir(old, &staging)?;
    std::fs::rename(&staging, new)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            // Preserves the file mode, so credentials.json stays 0600.
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Runs first thing in `run()`, before the database, the credential store or
/// the artwork protocol touch the new directories. A failure is logged and the
/// app starts fresh rather than failing to launch.
pub fn migrate_legacy_dirs() {
    for (old, new) in pairs() {
        if !should_copy(old.is_dir(), new.exists()) {
            continue;
        }
        match copy_dir_atomic(&old, &new) {
            Ok(()) => tracing::info!("Copied {} to {}", old.display(), new.display()),
            Err(e) => tracing::error!(
                "Could not copy {} to {}: {e}; starting without it",
                old.display(),
                new.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_only_when_the_old_directory_exists_and_the_new_does_not() {
        assert!(should_copy(true, false));
        assert!(!should_copy(false, false), "fresh install: nothing to copy");
        assert!(
            !should_copy(true, true),
            "already migrated: never overwrite"
        );
        assert!(!should_copy(false, true));
    }

    #[test]
    fn the_copy_leaves_the_old_directory_and_keeps_nested_files() {
        let root = std::env::temp_dir().join(format!("bragi-app-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (old, new) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(old.join("nested")).unwrap();
        std::fs::write(old.join("bragi.db"), b"db").unwrap();
        std::fs::write(old.join("nested").join("a.jpg"), b"img").unwrap();

        copy_dir_atomic(&old, &new).unwrap();

        assert_eq!(std::fs::read(new.join("bragi.db")).unwrap(), b"db");
        assert_eq!(
            std::fs::read(new.join("nested").join("a.jpg")).unwrap(),
            b"img"
        );
        assert!(old.join("bragi.db").exists(), "copy, never move");
        assert!(!root.join("new.migrating").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_constant_matches_the_tauri_identifier() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], APP_ID);
    }
}

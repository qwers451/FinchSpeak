//! Storage for server secrets (TLS private key, JWT signing secret) and the database backups,
//! which hold password hashes.
//!
//! Secrets are written atomically (temp file in the same directory, then rename) and, on Unix,
//! are readable by the owner only (mode 0600). If the directory does not allow that (read-only
//! directory, single-file bind mount), the file is written in place instead and a warning is
//! logged, so startup does not fail because of the directory. Files left behind by older
//! versions are tightened at startup with [`tighten_permissions`] and [`tighten_directory`].

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Write `bytes` to `path` atomically: the content goes to a temp file next to `path`, which is
/// then renamed over it. On Unix the file is owner-only (0600), also when it replaces an older
/// world-readable one. If the temp-file route fails, the file is written in place instead.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match write_via_temp(path, bytes) {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!("Could not write {path:?} through a temporary file ({e}); writing it in place");
            write_in_place(path, bytes)
        }
    }
}

/// Create `path` as an empty owner-only file (0600 on Unix). Fails if `path` already exists.
pub fn create_owner_only_file(path: &Path) -> io::Result<()> {
    create_private(path).map(|_| ())
}

/// Create `path` as a directory readable by its owner only (0700 on Unix), parents included.
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    make_private_dir(path)
}

/// Restrict an existing secret file to its owner (0600) on Unix when it is more open than that.
/// Returns `true` when the mode was changed; the change is logged as a warning. A missing file
/// has nothing to tighten and returns `false`.
pub fn tighten_permissions(path: &Path) -> io::Result<bool> {
    tighten_to(path, 0o600)
}

/// Restrict an existing directory to its owner (0700) on Unix when others or the group can enter
/// it. Returns `true` when the mode was changed. A missing directory returns `false`.
pub fn tighten_directory(path: &Path) -> io::Result<bool> {
    tighten_to(path, 0o700)
}

fn write_via_temp(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = temp_path(path);
    // A stale temp file from an earlier crash must not keep its old mode, so start clean.
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let written = write_new_then_rename(&tmp, path, bytes);
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

fn write_new_then_rename(tmp: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = create_private(tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(tmp, path)?;
    restrict(path)
}

/// Fallback: truncate and write `path` itself. The mode is restricted before the content is
/// written, so the secret is never readable by others.
fn write_in_place(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = open_in_place(path)?;
    restrict(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(unix)]
fn create_private(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(unix)]
fn open_in_place(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)
}

#[cfg(not(unix))]
fn open_in_place(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create(true).truncate(true).open(path)
}

#[cfg(unix)]
fn make_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path)
}

#[cfg(not(unix))]
fn make_private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

fn restrict(path: &Path) -> io::Result<()> {
    set_mode(path, 0o600)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn tighten_to(path: &Path, target: u32) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let mode = match std::fs::metadata(path) {
        Ok(meta) => meta.permissions().mode() & 0o777,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if mode & 0o077 == 0 {
        return Ok(false);
    }
    tracing::warn!("{path:?} was accessible to other users (mode {mode:o}); restricting it to its owner");
    set_mode(path, target)?;
    Ok(true)
}

#[cfg(not(unix))]
fn tighten_to(_path: &Path, _target: u32) -> io::Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_private_writes_exact_bytes_and_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");

        write_private(&path, b"first secret").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first secret");

        write_private(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "no temp file may be left behind: {leftovers:?}");
    }

    #[test]
    fn write_private_falls_back_when_the_temp_name_is_taken() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");
        // A directory under the temp name makes the temp-file route fail; the write must still land.
        std::fs::create_dir(temp_path(&path)).unwrap();

        write_private(&path, b"fallback").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"fallback");
        assert!(temp_path(&path).is_dir(), "the unrelated directory is left alone");
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    fn set_mode_for_test(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_private_creates_owner_only_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.der");
        write_private(&path, b"key").unwrap();
        assert_eq!(mode_of(&path), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn write_private_fixes_mode_of_existing_world_readable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.der");
        std::fs::write(&path, b"old").unwrap();
        set_mode_for_test(&path, 0o644);

        write_private(&path, b"new").unwrap();
        assert_eq!(mode_of(&path), 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn write_private_fallback_keeps_the_file_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.der");
        std::fs::write(&path, b"old").unwrap();
        set_mode_for_test(&path, 0o644);
        std::fs::create_dir(temp_path(&path)).unwrap();

        write_private(&path, b"new").unwrap();
        assert_eq!(mode_of(&path), 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn tighten_permissions_fixes_existing_file_at_startup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jwt_secret");
        std::fs::write(&path, b"secret").unwrap();
        set_mode_for_test(&path, 0o644);

        assert!(tighten_permissions(&path).unwrap());
        assert_eq!(mode_of(&path), 0o600);
        assert!(!tighten_permissions(&path).unwrap(), "already owner-only: nothing to change");
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
    }

    #[cfg(unix)]
    #[test]
    fn tighten_directory_removes_group_and_other_access() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        std::fs::create_dir(&backups).unwrap();
        set_mode_for_test(&backups, 0o755);

        assert!(tighten_directory(&backups).unwrap());
        assert_eq!(mode_of(&backups), 0o700);
        assert!(!tighten_directory(&dir.path().join("missing")).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn create_private_dir_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("a").join("backups");
        create_private_dir(&backups).unwrap();
        assert_eq!(mode_of(&backups), 0o700);
    }

    #[cfg(not(unix))]
    #[test]
    fn tighten_permissions_is_a_no_op_off_unix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jwt_secret");
        std::fs::write(&path, b"secret").unwrap();
        assert!(!tighten_permissions(&path).unwrap());
    }

    #[test]
    fn create_owner_only_file_makes_an_empty_file_that_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.db");
        create_owner_only_file(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap().len(), 0);
        assert!(create_owner_only_file(&path).is_err(), "must not replace an existing file");
    }
}

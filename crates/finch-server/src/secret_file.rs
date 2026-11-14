//! Storage for server secrets (TLS private key, JWT signing secret).
//!
//! Secrets are written atomically (temp file in the same directory, then rename) and, on Unix,
//! are readable by the owner only (mode 0600). Files left behind by older versions are tightened
//! at startup with [`tighten_permissions`].

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Write `bytes` to `path` atomically: the content goes to a temp file next to `path`, which is
/// then renamed over it. On Unix the file is owner-only (0600), also when it replaces an older
/// world-readable one.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = temp_path(path);
    // A stale temp file from an earlier crash must not keep its old mode, so start clean.
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    let written = (|| -> io::Result<()> {
        let mut file = create_private(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        restrict(path)
    })();

    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Restrict an existing secret file to its owner (0600) on Unix when it is more open than that.
/// Returns `true` when the mode was changed; the change is logged as a warning. A missing file
/// has nothing to tighten and returns `false`.
pub fn tighten_permissions(path: &Path) -> io::Result<bool> {
    tighten(path)
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
fn restrict(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn tighten(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let mode = match std::fs::metadata(path) {
        Ok(meta) => meta.permissions().mode(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if mode & 0o077 == 0 {
        return Ok(false);
    }
    tracing::warn!(
        "{path:?} was readable by other users (mode {:o}); restricting it to its owner",
        mode & 0o777
    );
    restrict(path)?;
    Ok(true)
}

#[cfg(not(unix))]
fn tighten(_path: &Path) -> io::Result<bool> {
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

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
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
        set_mode(&path, 0o644);

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
        set_mode(&path, 0o644);

        assert!(tighten_permissions(&path).unwrap());
        assert_eq!(mode_of(&path), 0o600);
        assert!(!tighten_permissions(&path).unwrap(), "already owner-only: nothing to change");
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
    }

    #[cfg(not(unix))]
    #[test]
    fn tighten_permissions_is_a_no_op_off_unix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jwt_secret");
        std::fs::write(&path, b"secret").unwrap();
        assert!(!tighten_permissions(&path).unwrap());
    }
}

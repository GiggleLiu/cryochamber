//! Atomic replacement of small configuration and runtime files.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    replace(path, contents.as_ref(), false)
}

pub(crate) fn write_durable(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    replace(path, contents.as_ref(), true)
}

fn replace(path: &Path, contents: &[u8], durable: bool) -> std::io::Result<()> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_file_name(format!(
        ".{name}.tmp-{}-{seq}",
        crate::state::new_instance_id()
    ));
    // Set permissions at creation, before writing possible provider secrets.
    // create_new refuses symlinks and collisions without touching their target.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let result = (|| {
        file.write_all(contents)?;
        if durable {
            file.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        if durable {
            std::fs::File::open(path.parent().unwrap_or(Path::new(".")))?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn atomic_replace_preserves_old_readers_and_cleans_failed_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        std::fs::write(&path, "old").unwrap();
        let old = std::fs::File::open(&path).unwrap();
        write_atomic(&path, "secret").unwrap();
        assert_eq!(std::io::read_to_string(old).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "secret");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let occupied = dir.path().join("directory");
        std::fs::create_dir(&occupied).unwrap();
        assert!(write_atomic(&occupied, "replacement").is_err());
        assert!(occupied.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}

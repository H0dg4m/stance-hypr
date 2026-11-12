//! Filesystem helpers: XDG paths, private directories, atomic writes,
//! the state lock and size/permission-checked reads.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::hypr::current_uid;

/// Mode files, rules and other user input are capped at this size.
pub const MAX_INPUT: u64 = 64 * 1024;

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| Error::new("env", "HOME is not set"))
}

fn xdg(var: &str, fallback: &str) -> Result<PathBuf> {
    match std::env::var_os(var).map(PathBuf::from) {
        Some(p) if p.is_absolute() => Ok(p),
        _ => Ok(home()?.join(fallback)),
    }
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(xdg("XDG_CONFIG_HOME", ".config")?.join("stance"))
}

pub fn hypr_config_dir() -> Result<PathBuf> {
    Ok(xdg("XDG_CONFIG_HOME", ".config")?.join("hypr"))
}

pub fn state_dir() -> Result<PathBuf> {
    Ok(xdg("XDG_STATE_HOME", ".local/state")?.join("stance"))
}

/// Creates `dir` (0700) if needed and checks it is ours and private enough
/// that nobody else can swap files in it.
pub fn ensure_private_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| Error::new("io", format!("cannot create {}: {e}", dir.display())))?;
    }
    check_dir(dir)
}

/// Directory must be a real directory owned by us, not group/world writable.
pub fn check_dir(dir: &Path) -> Result<()> {
    crate::hypr::check_private_dir(dir, current_uid()?)
        .map_err(|e| Error::new("unsafe_path", e.message))
}

/// Writes `data` to `path` atomically: exclusive temp file (0600, no
/// symlink follow) in the same directory, fsync, rename. Refuses to replace
/// a symlink. The previous file, if any, is kept as `<name>.bak`.
pub fn atomic_write(path: &Path, data: &[u8], keep_backup: bool) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::new("io", "path has no parent"))?;
    check_dir(dir)?;
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        return Err(Error::new(
            "unsafe_path",
            format!("{} is a symlink; refusing to replace it", path.display()),
        ));
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::new("io", "bad file name"))?;
    // pid + counter: threads of one process never share a temp name.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let io = |e: std::io::Error| Error::new("io", format!("{}: {e}", path.display()));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .map_err(io)?;
    let res = f.write_all(data).and_then(|_| f.sync_all());
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        return Err(io(e));
    }
    if keep_backup && path.exists() {
        let _ = fs::copy(path, dir.join(format!("{name}.bak")));
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io(e)
    })?;
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Reads a user-provided file with a size cap. The file must be owned by
/// us and not writable by group or others.
pub fn read_checked(path: &Path) -> Result<String> {
    let io = |e: std::io::Error| Error::new("io", format!("{}: {e}", path.display()));
    let f = File::open(path).map_err(io)?;
    let meta = f.metadata().map_err(io)?;
    if !meta.is_file() {
        return Err(Error::new(
            "unsafe_path",
            format!("{} is not a regular file", path.display()),
        ));
    }
    if meta.uid() != current_uid()? || meta.permissions().mode() & 0o022 != 0 {
        return Err(Error::new(
            "unsafe_path",
            format!(
                "{} must be owned by you and not group/world writable (fix: chmod go-w '{}')",
                path.display(),
                path.display()
            ),
        ));
    }
    if meta.len() > MAX_INPUT {
        return Err(Error::new(
            "too_large",
            format!("{} is larger than 64 KiB", path.display()),
        ));
    }
    let mut s = String::new();
    f.take(MAX_INPUT + 1).read_to_string(&mut s).map_err(io)?;
    Ok(s)
}

/// Exclusive lock over all state changes; released on drop.
pub struct StateLock(#[allow(dead_code)] File);

pub fn lock_state_in(dir: &Path) -> Result<StateLock> {
    lock_named(dir, "lock")
}

/// Exclusive lock on `<dir>/<name>` (separate lock files for state that is
/// changed independently, e.g. a config next to an index).
pub fn lock_named(dir: &Path, name: &str) -> Result<StateLock> {
    ensure_private_dir(dir)?;
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(name))
        .map_err(|e| Error::new("io", format!("state lock: {e}")))?;
    f.lock()
        .map_err(|e| Error::new("io", format!("state lock: {e}")))?;
    Ok(StateLock(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stance-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        ensure_private_dir(&d).unwrap();
        d
    }

    #[test]
    fn atomic_write_and_backup() {
        let d = tmpdir("aw");
        let p = d.join("f.txt");
        atomic_write(&p, b"one", true).unwrap();
        atomic_write(&p, b"two", true).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "two");
        assert_eq!(fs::read_to_string(d.join("f.txt.bak")).unwrap(), "one");
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn refuses_symlink_target() {
        let d = tmpdir("sl");
        std::os::unix::fs::symlink("/etc/passwd", d.join("x")).unwrap();
        assert!(atomic_write(&d.join("x"), b"no", false).is_err());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn read_checked_rules() {
        let d = tmpdir("rc");
        let p = d.join("m.toml");
        fs::write(&p, "a = 1").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_checked(&p).is_ok());
        fs::set_permissions(&p, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(read_checked(&p).is_err());
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&p, vec![b'#'; 70 * 1024]).unwrap();
        assert!(read_checked(&p).is_err());
        fs::remove_dir_all(&d).ok();
    }
}

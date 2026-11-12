//! Minimal Hyprland IPC client (socket1, request/response).
//!
//! No `hyprctl`, no shell. The socket path is built from validated
//! environment values and checked with `lstat` before connecting.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};

/// Replies larger than this are treated as an error, not buffered forever.
const MAX_REPLY: u64 = 4 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// `HYPRLAND_INSTANCE_SIGNATURE` looks like `<commit>_<time>_<rand>`.
/// Anything outside `[A-Za-z0-9_-]` could escape the runtime dir.
pub fn valid_signature(sig: &str) -> bool {
    !sig.is_empty()
        && sig.len() <= 256
        && sig
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Effective UID of this process.
pub fn current_uid() -> Result<u32> {
    // SAFETY: geteuid has no preconditions and cannot fail.
    Ok(unsafe { libc::geteuid() })
}

/// Checks that `dir` is a real directory owned by `uid` and not writable
/// by group or others.
pub fn check_private_dir(dir: &Path, uid: u32) -> Result<()> {
    let meta = fs::symlink_metadata(dir)
        .map_err(|e| Error::new("runtime_dir", format!("{}: {e}", dir.display())))?;
    if !meta.file_type().is_dir() {
        return Err(Error::new(
            "runtime_dir",
            format!("{} is not a directory", dir.display()),
        ));
    }
    if meta.uid() != uid {
        return Err(Error::new(
            "runtime_dir",
            format!("{} is owned by uid {}", dir.display(), meta.uid()),
        ));
    }
    if meta.permissions().mode() & 0o022 != 0 {
        return Err(Error::new(
            "runtime_dir",
            format!(
                "{} is group/world writable (fix: chmod go-w '{}')",
                dir.display(),
                dir.display()
            ),
        ));
    }
    Ok(())
}

pub fn runtime_dir() -> Result<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| Error::new("runtime_dir", "XDG_RUNTIME_DIR is not set or not absolute"))
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub signature: String,
    pub dir: PathBuf,
}

impl Instance {
    /// Locates the running Hyprland instance from the environment.
    pub fn from_env() -> Result<Self> {
        let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").map_err(|_| {
            Error::new(
                "not_hyprland",
                "HYPRLAND_INSTANCE_SIGNATURE is not set; is Hyprland running?",
            )
        })?;
        if !valid_signature(&sig) {
            return Err(Error::new(
                "bad_signature",
                "HYPRLAND_INSTANCE_SIGNATURE contains unexpected characters",
            ));
        }
        let runtime = runtime_dir()?;
        Ok(Self {
            dir: runtime.join("hypr").join(&sig),
            signature: sig,
        })
    }

    /// Like [`Instance::from_env`], but when the variable is missing (e.g. a
    /// systemd user unit started before Hyprland exported it) picks the
    /// newest instance directory owned by this user.
    pub fn discover() -> Result<Self> {
        match Self::from_env() {
            Err(e) if e.code == "not_hyprland" => {}
            other => return other,
        }
        let runtime = runtime_dir()?;
        let uid = current_uid()?;
        let newest = fs::read_dir(runtime.join("hypr"))
            .map_err(|_| Error::new("not_hyprland", "Hyprland does not seem to be running"))?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let meta = fs::symlink_metadata(e.path()).ok()?;
                (valid_signature(&name)
                    && meta.is_dir()
                    && meta.uid() == uid
                    && e.path().join(".socket.sock").exists())
                .then(|| (meta.mtime(), name))
            })
            .max();
        let (_, sig) = newest
            .ok_or_else(|| Error::new("not_hyprland", "Hyprland does not seem to be running"))?;
        Ok(Self {
            dir: runtime.join("hypr").join(&sig),
            signature: sig,
        })
    }

    /// Opens socket2 for line-based events (`name>>data`).
    pub fn events(&self) -> Result<std::io::BufReader<UnixStream>> {
        let path = self.socket2();
        self.check_socket(&path)?;
        let s = UnixStream::connect(&path)
            .map_err(|e| Error::new("connect", format!("{}: {e}", path.display())))?;
        Ok(std::io::BufReader::new(s))
    }

    pub fn socket1(&self) -> PathBuf {
        self.dir.join(".socket.sock")
    }

    pub fn socket2(&self) -> PathBuf {
        self.dir.join(".socket2.sock")
    }

    /// Verifies that `path` is a socket owned by us inside a private dir.
    pub fn check_socket(&self, path: &Path) -> Result<()> {
        let uid = current_uid()?;
        check_private_dir(&self.dir, uid)?;
        let meta = fs::symlink_metadata(path)
            .map_err(|e| Error::new("socket", format!("{}: {e}", path.display())))?;
        if !meta.file_type().is_socket() {
            return Err(Error::new(
                "socket",
                format!("{} is not a socket", path.display()),
            ));
        }
        if meta.uid() != uid {
            return Err(Error::new(
                "socket",
                format!("{} is owned by uid {}", path.display(), meta.uid()),
            ));
        }
        Ok(())
    }

    /// Sends one request on socket1 and returns the full reply.
    pub fn request(&self, req: &str) -> Result<String> {
        self.request_on(&self.socket1(), req)
    }

    /// Same request/reply exchange on another socket in the instance dir
    /// (hyprsunset speaks the same framing on `.hyprsunset.sock`).
    pub fn request_on(&self, path: &Path, req: &str) -> Result<String> {
        self.check_socket(path)?;
        let mut stream = UnixStream::connect(path)
            .map_err(|e| Error::new("connect", format!("{}: {e}", path.display())))?;
        stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
        stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
        stream
            .write_all(req.as_bytes())
            .map_err(|e| Error::new("ipc", format!("write: {e}")))?;
        stream.shutdown(std::net::Shutdown::Write).ok();

        let mut buf = Vec::new();
        (&mut stream)
            .take(MAX_REPLY + 1)
            .read_to_end(&mut buf)
            .map_err(|e| Error::new("ipc", format!("read: {e}")))?;
        if buf.len() as u64 > MAX_REPLY {
            return Err(Error::new("ipc", "reply too large"));
        }
        String::from_utf8(buf).map_err(|_| Error::new("ipc", "reply is not UTF-8"))
    }

    /// JSON request (`j/<cmd>`), parsed into `T`.
    pub fn request_json<T: serde::de::DeserializeOwned>(&self, cmd: &str) -> Result<T> {
        let reply = self.request(&format!("j/{cmd}"))?;
        serde_json::from_str(&reply)
            .map_err(|e| Error::new("ipc", format!("unexpected reply to {cmd}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_rules() {
        assert!(valid_signature(
            "acc1aa0bfd4bd575dcb0f76c35ccf12b660a1ab9_1794247061_1357641769"
        ));
        assert!(!valid_signature(""));
        assert!(!valid_signature("../../etc"));
        assert!(!valid_signature("a/b"));
        assert!(!valid_signature("a b"));
        assert!(!valid_signature("a\nb"));
        assert!(!valid_signature(&"a".repeat(300)));
    }
}

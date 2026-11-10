//! Running external tools: absolute path only, no shell, stdin null,
//! hard timeout (kill + wait), sanitized stderr.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// Finds `name` at `/usr/bin/<name>`, then in absolute PATH entries.
pub fn find(name: &str) -> Option<PathBuf> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    {
        return None;
    }
    let fixed = PathBuf::from("/usr/bin").join(name);
    if fixed.is_file() {
        return Some(fixed);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_absolute() && p.is_file())
}

/// Runs `path args...`, returns stdout. Fails on non-zero exit (message from
/// sanitized stderr) and on timeout (child is killed and reaped).
pub fn run_tool(path: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let label = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "tool".into());
    let mut child = Command::new(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::new("tool", format!("{label}: {e}")))?;
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let t_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(s) = so.as_mut() {
            let _ = s.take(4 << 20).read_to_end(&mut b);
        }
        b
    });
    let t_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(s) = se.as_mut() {
            let _ = s.take(64 << 10).read_to_end(&mut b);
        }
        b
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = t_out.join();
                let _ = t_err.join();
                return Err(Error::new(
                    "tool_timeout",
                    format!("{label} timed out after {}s", timeout.as_secs_f32()),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => break Err(e),
        }
    };
    let out = t_out.join().unwrap_or_default();
    let err = t_err.join().unwrap_or_default();
    let status = status.map_err(|e| Error::new("tool", format!("{label}: {e}")))?;
    if !status.success() {
        return Err(Error::new(
            "tool",
            format!(
                "{label} failed: {}",
                crate::model::sanitize(String::from_utf8_lossy(&err).trim())
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_times_out() {
        let sh = Path::new("/usr/bin/sleep");
        if !sh.is_file() {
            return;
        }
        let r = run_tool(sh, &["5"], Duration::from_millis(100));
        assert_eq!(r.unwrap_err().code, "tool_timeout");
        assert!(run_tool(sh, &["0"], Duration::from_secs(5)).is_ok());
        assert!(find("a/b").is_none() && find("").is_none() && find("a b").is_none());
    }
}

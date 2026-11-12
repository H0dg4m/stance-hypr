//! Kernel uevents (NETLINK_KOBJECT_UEVENT) for power-supply changes, so
//! the daemon notices the charger being plugged in or out. Only used as a
//! wake-up: the state itself is read from sysfs, so a forged message could
//! at most cause a harmless re-check.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Starts a thread that sets `flag` on every power_supply uevent. Returns
/// false when the netlink socket is unavailable (then power rules only
/// apply on monitor hotplug).
pub fn watch_power(flag: Arc<AtomicBool>) -> bool {
    // SAFETY: plain socket/bind calls with a zeroed, correctly sized
    // sockaddr_nl; the fd is owned by the thread below for its lifetime.
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            libc::NETLINK_KOBJECT_UEVENT,
        )
    };
    if fd < 0 {
        return false;
    }
    let mut addr: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    addr.nl_groups = 1;
    let ok = unsafe {
        libc::bind(
            fd,
            (&addr as *const libc::sockaddr_nl).cast(),
            std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    } == 0;
    if !ok {
        unsafe { libc::close(fd) };
        return false;
    }
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 8192];
        loop {
            // SAFETY: buf is valid for buf.len() bytes.
            let n = unsafe { libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0) };
            if n < 0 {
                match std::io::Error::last_os_error().raw_os_error() {
                    Some(libc::EINTR) => continue,
                    // Events were dropped while we were slow: one of them
                    // may have been the charger, so look again.
                    Some(libc::ENOBUFS) => {
                        flag.store(true, Ordering::Relaxed);
                        continue;
                    }
                    _ => break,
                }
            }
            if is_power_event(&buf[..n as usize]) {
                flag.store(true, Ordering::Relaxed);
            }
        }
        unsafe { libc::close(fd) };
    });
    true
}

/// `ACTION@DEVPATH\0KEY=VALUE\0...` with SUBSYSTEM=power_supply.
pub fn is_power_event(msg: &[u8]) -> bool {
    msg.split(|b| *b == 0)
        .any(|f| f == b"SUBSYSTEM=power_supply")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_power_supply_events() {
        let ev = b"change@/devices/LNXSYSTM:00/ACAD/power_supply/ACAD\0ACTION=change\0SUBSYSTEM=power_supply\0POWER_SUPPLY_ONLINE=1\0";
        assert!(is_power_event(ev));
        assert!(!is_power_event(b"add@/devices/usb\0SUBSYSTEM=usb\0"));
        assert!(!is_power_event(b"SUBSYSTEM=power_supplyX\0"));
    }
}

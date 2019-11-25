use error_chain::bail;
use libc::pid_t;
use nix::unistd;
use std::os::unix::net::UnixDatagram;
use std::{env, fmt, fs, path, time};

use crate::errors::*;

/// Check for systemd presence at runtime.
///
/// Return true if the system was booted with systemd.
/// This check is based on the presence of the systemd
/// runtime directory.
pub fn booted() -> bool {
    fs::symlink_metadata("/run/systemd/system")
        .map(|p| p.is_dir())
        .unwrap_or(false)
}

/// Check for watchdog support at runtime.
///
/// Return a timeout before which the watchdog expects a
/// response from the process, or `None` if watchdog support is
/// not enabled. If `unset_env` is true, environment will be cleared.
pub fn watchdog_enabled(unset_env: bool) -> Option<time::Duration> {
    let env_usec = env::var("WATCHDOG_USEC").ok();
    let env_pid = env::var("WATCHDOG_PID").ok();

    if unset_env {
        env::remove_var("WATCHDOG_USEC");
        env::remove_var("WATCHDOG_PID");
    };

    let timeout = {
        if let Some(usec) = env_usec.and_then(|usec_str| usec_str.parse::<u64>().ok()) {
            time::Duration::from_millis(usec / 1_000)
        } else {
            return None;
        }
    };

    let pid = {
        if let Some(pid_str) = env_pid {
            if let Ok(p) = pid_str.parse::<pid_t>() {
                unistd::Pid::from_raw(p)
            } else {
                return None;
            }
        } else {
            return Some(timeout);
        }
    };

    if unistd::getpid() == pid {
        Some(timeout)
    } else {
        None
    }
}

/// Notify service manager about status changes.
///
/// Send a notification to the manager about service status changes.
/// The returned boolean show whether notifications are supported for
/// this service. If `unset_env` is true, environment will be cleared
/// and no further notifications are possible.
pub fn notify(unset_env: bool, state: &[NotifyState]) -> Result<bool> {
    let env_sock = env::var("NOTIFY_SOCKET").ok();

    if unset_env {
        env::remove_var("NOTIFY_SOCKET");
    };

    let path = {
        if let Some(p) = env_sock.map(path::PathBuf::from) {
            p
        } else {
            return Ok(false);
        }
    };
    let sock = UnixDatagram::unbound()?;

    let msg = state
        .iter()
        .fold(String::new(), |res, s| res + &format!("{}\n", s));
    let msg_len = msg.len();
    let sent_len = sock.send_to(msg.as_bytes(), path)?;
    if sent_len != msg_len {
        bail!("incomplete write, sent {} out of {}", sent_len, msg_len);
    }
    Ok(true)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// Status changes, see `sd_notify(3)`.
pub enum NotifyState {
    /// D-Bus error-style error code.
    Buserror(String),
    /// errno-style error code.
    Errno(u8),
    /// A name for the submitted file descriptors.
    Fdname(String),
    /// Stores additional file descriptors in the service manager.
    Fdstore,
    /// The main process ID of the service, in case of forking applications.
    Mainpid(unistd::Pid),
    /// Custom state change, as a `KEY=VALUE` string.
    Other(String),
    /// Service startup is finished.
    Ready,
    /// Service is reloading.
    Reloading,
    /// Custom status change.
    Status(String),
    /// Service is beginning to shutdown.
    Stopping,
    /// Tell the service manager to update the watchdog timestamp.
    Watchdog,
    /// Reset watchdog timeout value during runtime.
    WatchdogUsec(u64),
}

impl fmt::Display for NotifyState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            NotifyState::Buserror(ref s) => write!(f, "BUSERROR={}", s),
            NotifyState::Errno(e) => write!(f, "ERRNO={}", e),
            NotifyState::Fdname(ref s) => write!(f, "FDNAME={}", s),
            NotifyState::Fdstore => write!(f, "FDSTORE=1"),
            NotifyState::Mainpid(ref p) => write!(f, "MAINPID={}", p),
            NotifyState::Other(ref s) => write!(f, "{}", s),
            NotifyState::Ready => write!(f, "READY=1"),
            NotifyState::Reloading => write!(f, "RELOADING=1"),
            NotifyState::Status(ref s) => write!(f, "STATUS={}", s),
            NotifyState::Stopping => write!(f, "STOPPING=1"),
            NotifyState::Watchdog => write!(f, "WATCHDOG=1"),
            NotifyState::WatchdogUsec(u) => write!(f, "WATCHDOG_USEC={}", u),
        }
    }
}

use std::{env, fmt, fs, path, time};
use std::os::unix::net::UnixDatagram;
use libc::pid_t;
use nix::unistd;

use errors::*;

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
        let usec_str = try_opt_or!(env_usec, None);
        let usec = try_opt_or!(usec_str.parse::<u64>().ok(), None);
        time::Duration::from_millis(usec / 1_000)
    };

    let pid = {
        let pid_str = try_opt_or!(env_pid, Some(timeout));
        let p = try_opt_or!(pid_str.parse::<pid_t>().ok(), None);
        unistd::Pid::from_raw(p)
    };

    match unistd::getpid() == pid {
        true => Some(timeout),
        false => None,
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
        let s = try_opt_or!(env_sock, Ok(false));
        path::PathBuf::from(s)
    };
    let sock = UnixDatagram::unbound()?;

    let msg = state.iter().fold(String::new(), |res, &ref s| {
        res + &format!("{}\n", s)
    });
    let msg_len = msg.len();
    let sent_len = sock.send_to(msg.as_bytes(), path)?;
    if sent_len != msg_len {
        bail!("incomplete write, sent {} out of {}", sent_len, msg_len);
    }
    Ok(true)
}

#[derive(Clone, Debug)]
/// Status changes, see `sd_notify(3)`.
pub enum NotifyState {
    Buserror(String),
    Errno(u8),
    Fdname(String),
    Fdstore,
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
    WatchdogUsec(u64),
}

impl fmt::Display for NotifyState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg = match self {
            &NotifyState::Buserror(ref s) => format!("BUSERROR={}", s),
            &NotifyState::Errno(e) => format!("ERRNO={}", e),
            &NotifyState::Fdname(ref s) => format!("FDNAME={}", s),
            &NotifyState::Fdstore => format!("FDSTORE=1"),
            &NotifyState::Mainpid(ref p) => format!("MAINPID={}", p),
            &NotifyState::Other(ref s) => s.clone(),
            &NotifyState::Ready => format!("READY=1"),
            &NotifyState::Reloading => format!("RELOADING=1"),
            &NotifyState::Status(ref s) => format!("STATUS={}", s),
            &NotifyState::Stopping => format!("STOPPING=1"),
            &NotifyState::Watchdog => format!("WATCHDOG=1"),
            &NotifyState::WatchdogUsec(u) => format!("WATCHDOG_USEC={}", u),
        };
        write!(f, "{}", msg)
    }
}

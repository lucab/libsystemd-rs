use crate::errors::{Context as _, SdError};
use libc::pid_t;
use nix::sys::socket;
use nix::unistd;
use std::fmt::Write as _;
use std::io::{self, IoSlice};
use std::os::unix::io::RawFd;
use std::os::unix::net::UnixDatagram;
use std::os::unix::prelude::AsRawFd as _;
use std::{env, fmt, fs, time};

/// Check for systemd presence at runtime.
///
/// Return true if the system was booted with systemd.
/// This check is based on the presence of the systemd
/// runtime directory.
#[must_use]
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
#[must_use]
pub fn watchdog_enabled(unset_env: bool) -> Option<time::Duration> {
    let env_usec = env::var("WATCHDOG_USEC").ok();
    let env_pid = env::var("WATCHDOG_PID").ok();

    if unset_env {
        unsafe { env::remove_var("WATCHDOG_USEC") };
        unsafe { env::remove_var("WATCHDOG_PID") };
    }

    let timeout = {
        if let Some(usec) = env_usec.and_then(|usec_str| usec_str.parse::<u64>().ok()) {
            time::Duration::from_micros(usec)
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
/// Also see [`notify_with_fds`] which can send file descriptors to the
/// service manager.
pub fn notify(unset_env: bool, state: &[NotifyState]) -> Result<bool, SdError> {
    notify_with_fds(unset_env, state, &[])
}

/// Notify service manager about status changes and send file descriptors.
///
/// Use this together with [`NotifyState::Fdstore`]. Otherwise works like [`notify`].
pub fn notify_with_fds(
    unset_env: bool,
    state: &[NotifyState],
    fds: &[RawFd],
) -> Result<bool, SdError> {
    let Ok(env_sock) = env::var("NOTIFY_SOCKET") else {
        return Ok(false);
    };

    if unset_env {
        unsafe { env::remove_var("NOTIFY_SOCKET") };
    }

    sanity_check_state_entries(state)?;

    // If the first character of `$NOTIFY_SOCKET` is '@', the string
    // is understood as Linux abstract namespace socket.
    let socket_addr = match env_sock.strip_prefix('@') {
        Some(stripped_addr) => socket::UnixAddr::new_abstract(stripped_addr.as_bytes())
            .with_context(|| format!("invalid Unix socket abstract address {env_sock}"))?,
        None => socket::UnixAddr::new(env_sock.as_str())
            .with_context(|| format!("invalid Unix socket path address {env_sock}"))?,
    };

    let socket = UnixDatagram::unbound().context("failed to open Unix datagram socket")?;
    let msg = state
        .iter()
        .fold(String::new(), |mut res, s| {
            writeln!(res, "{s}").unwrap();
            res
        })
        .into_bytes();
    let msg_len = msg.len();
    let msg_iov = IoSlice::new(&msg);

    let ancillary = if fds.is_empty() {
        vec![]
    } else {
        vec![socket::ControlMessage::ScmRights(fds)]
    };

    let sent_len = socket::sendmsg(
        socket.as_raw_fd(),
        &[msg_iov],
        &ancillary,
        socket::MsgFlags::empty(),
        Some(&socket_addr),
    )
    .map_err(|e| io::Error::from_raw_os_error(e as i32))
    .context("failed to send notify datagram")?;

    if sent_len != msg_len {
        return Err(format!("incomplete notify sendmsg, sent {sent_len} out of {msg_len}").into());
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
    /// Stores additional file descriptors in the service manager. Use [`notify_with_fds`] with this.
    Fdstore,
    /// Remove stored file descriptors. Must be used together with [`NotifyState::Fdname`].
    FdstoreRemove,
    /// Tell the service manager to not poll the filedescriptors for errors. This causes
    /// systemd to hold on to broken file descriptors which must be removed manually.
    /// Must be used together with [`NotifyState::Fdstore`].
    FdpollDisable,
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
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Buserror(ref s) => write!(f, "BUSERROR={s}"),
            Self::Errno(e) => write!(f, "ERRNO={e}"),
            Self::Fdname(ref s) => write!(f, "FDNAME={s}"),
            Self::Fdstore => write!(f, "FDSTORE=1"),
            Self::FdstoreRemove => write!(f, "FDSTOREREMOVE=1"),
            Self::FdpollDisable => write!(f, "FDPOLL=0"),
            Self::Mainpid(ref p) => write!(f, "MAINPID={p}"),
            Self::Other(ref s) => write!(f, "{s}"),
            Self::Ready => write!(f, "READY=1"),
            Self::Reloading => write!(f, "RELOADING=1"),
            Self::Status(ref s) => write!(f, "STATUS={s}"),
            Self::Stopping => write!(f, "STOPPING=1"),
            Self::Watchdog => write!(f, "WATCHDOG=1"),
            Self::WatchdogUsec(u) => write!(f, "WATCHDOG_USEC={u}"),
        }
    }
}

/// Perform some basic sanity checks against state entries.
fn sanity_check_state_entries(state: &[NotifyState]) -> Result<(), SdError> {
    for (index, entry) in state.iter().enumerate() {
        if let NotifyState::Fdname(ref name) = *entry {
            validate_fdname(name)
                .with_context(|| format!("invalid notify state entry #{index}"))?;
        }
    }

    Ok(())
}

/// Validate an `FDNAME` according to systemd rules.
///
/// The name may consist of arbitrary ASCII characters except control
/// characters or ":". It may not be longer than 255 characters.
fn validate_fdname(fdname: &str) -> Result<(), SdError> {
    if fdname.len() > 255 {
        return Err(format!("fdname '{fdname}' longer than 255 characters").into());
    }

    for c in fdname.chars() {
        if !c.is_ascii() || c == ':' || c.is_ascii_control() {
            return Err(format!("invalid character '{c}' in fdname '{fdname}'").into());
        }
    }

    Ok(())
}

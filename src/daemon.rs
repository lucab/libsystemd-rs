use libc::pid_t;
use nix::mqueue::mq_getattr;
use nix::sys::socket::getsockname;
use nix::sys::socket::SockAddr;
use nix::sys::stat::fstat;
use nix::unistd;
use std::convert::TryFrom;
use std::os::unix::io::RawFd;
use std::os::unix::net::UnixDatagram;
use std::process;
use std::{env, fmt, fs, path, time};

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

#[derive(Clone, Debug)]
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
        let msg = match *self {
            NotifyState::Buserror(ref s) => format!("BUSERROR={}", s),
            NotifyState::Errno(e) => format!("ERRNO={}", e),
            NotifyState::Fdname(ref s) => format!("FDNAME={}", s),
            NotifyState::Fdstore => "FDSTORE=1".to_string(),
            NotifyState::Mainpid(ref p) => format!("MAINPID={}", p),
            NotifyState::Other(ref s) => s.clone(),
            NotifyState::Ready => "READY=1".to_string(),
            NotifyState::Reloading => "RELOADING=1".to_string(),
            NotifyState::Status(ref s) => format!("STATUS={}", s),
            NotifyState::Stopping => "STOPPING=1".to_string(),
            NotifyState::Watchdog => "WATCHDOG=1".to_string(),
            NotifyState::WatchdogUsec(u) => format!("WATCHDOG_USEC={}", u),
        };
        write!(f, "{}", msg)
    }
}

const SD_LISTEN_FDS_START: RawFd = 3;

pub trait IsType {
    /// Returns true if a file descriptor is a FIFO.
    fn is_fifo(&self) -> bool;

    /// Returns true if a file descriptor is a special file.
    fn is_special(&self) -> bool;

    /// Returns true if a file descriptor is a PF_INET socket.
    fn is_inet(&self) -> bool;

    /// Returns true if a file descriptor is a PF_UNIX socket.
    fn is_unix(&self) -> bool;

    /// Returns true if a file descriptor is a POSIX message queue descriptor.
    fn is_mq(&self) -> bool;
}

/// The possible types of sockets passed to a socket activated daemon.
///
/// https://www.freedesktop.org/software/systemd/man/systemd.socket.html
#[derive(Debug, Clone)]
pub enum SocketType {
    /// A FIFO named pipe (see man 7 fifo)
    Fifo(RawFd),
    /// A special file, such as character device nodes or special files in
    /// /proc and /sys
    Special(RawFd),
    /// A PF_INET socket, such as UDP/TCP sockets
    Inet(RawFd),
    /// A PF_UNIX socket (see man 7 unix)
    Unix(RawFd),
    /// A POSIX message queue (see man 7 mq_overview)
    Mq(RawFd),
}

impl IsType for SocketType {
    fn is_fifo(&self) -> bool {
        match self {
            SocketType::Fifo(_) => true,
            _ => false,
        }
    }

    fn is_special(&self) -> bool {
        match self {
            SocketType::Special(_) => true,
            _ => false,
        }
    }

    fn is_unix(&self) -> bool {
        match self {
            SocketType::Unix(_) => true,
            _ => false,
        }
    }

    fn is_inet(&self) -> bool {
        match self {
            SocketType::Inet(_) => true,
            _ => false,
        }
    }

    fn is_mq(&self) -> bool {
        match self {
            SocketType::Mq(_) => true,
            _ => false,
        }
    }
}

/// Check for file descriptors passed by systemd
///
/// Invoked by socket activated daemons to check for file descriptors needed by the service.
/// If unset_env is true, the environment variables used by systemd will be cleared.
pub fn sd_listen_fds(unset_env: bool) -> Result<Vec<SocketType>> {
    let pid = env::var("LISTEN_PID")?;
    let fds = env::var("LISTEN_FDS")?;
    if unset_env {
        env::remove_var("LISTEN_PID");
        env::remove_var("LISTEN_FDS");
        env::remove_var("LISTEN_FDNAMES");
    }

    let pid = pid.parse::<u32>()?;
    let fds = fds.parse::<i32>()?;

    if process::id() != pid {
        return Err("Pid mismatch".into());
    }

    let vec = socks_from_fds(fds);
    Ok(vec)
}

/// Check for file descriptors passed by systemd
///
/// Like sd_listen_fds, but will also return a Vec of names associated with each file
/// descriptor.
pub fn sd_listen_fds_with_names(unset_env: bool) -> Result<Vec<(SocketType, String)>> {
    let pid = env::var("LISTEN_PID")?;
    let fds = env::var("LISTEN_FDS")?;
    let names = env::var("LISTEN_FDNAMES")?;

    if unset_env {
        env::remove_var("LISTEN_PID");
        env::remove_var("LISTEN_FDS");
        env::remove_var("LISTEN_FDNAMES");
    }

    let pid = pid.parse::<u32>()?;
    let fds = fds.parse::<i32>()?;

    if process::id() != pid {
        return Err("Pid mismatch".into());
    }

    let names: Vec<String> = names.split(":").map(String::from).collect();
    let vec = socks_from_fds(fds);
    let out = vec.into_iter().zip(names.into_iter()).collect();

    Ok(out)
}

fn socks_from_fds(num_fds: i32) -> Vec<SocketType> {
    let mut vec = Vec::new();
    for fd in SD_LISTEN_FDS_START..SD_LISTEN_FDS_START+num_fds {
        if let Ok(sock) = SocketType::try_from(fd) {
            vec.push(sock);
        } else {
            eprintln!("Socket conversion error");
        }
    }

    vec
}

impl IsType for RawFd {
    fn is_fifo(&self) -> bool {
        match fstat(*self) {
            Ok(stat) => (stat.st_mode & 0_170_000) == 10000,
            Err(_) => false,
        }
    }

    fn is_special(&self) -> bool {
        match fstat(*self) {
            Ok(stat) => (stat.st_mode & 0_170_000) == 100000,
            Err(_) => false,
        }
    }

    fn is_inet(&self) -> bool {
        match getsockname(*self) {
            Ok(addr) => {
                if let SockAddr::Inet(_) = addr {
                    return true;
                } else {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }

    fn is_unix(&self) -> bool {
        match getsockname(*self) {
            Ok(addr) => {
                if let SockAddr::Unix(_) = addr {
                    return true;
                } else {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }

    fn is_mq(&self) -> bool {
        mq_getattr(*self).is_ok()
    }
}

impl TryFrom<RawFd> for SocketType {
    type Error = &'static str;

    fn try_from(value: RawFd) -> std::result::Result<Self, Self::Error> {
        if value.is_fifo() {
            return Ok(SocketType::Fifo(value));
        } else if value.is_special() {
            return Ok(SocketType::Special(value));
        } else if value.is_inet() {
            return Ok(SocketType::Inet(value));
        } else if value.is_unix() {
            return Ok(SocketType::Unix(value));
        } else if value.is_mq() {
            return Ok(SocketType::Mq(value));
        }

        return Err("Invalid FD");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socketype_is_unix() {
        let sock = SocketType::Unix(0i32);
        assert!(sock.is_unix());
    }

    #[test]
    fn test_socketype_is_special() {
        let sock = SocketType::Special(0i32);
        assert!(sock.is_special());
    }

    #[test]
    fn test_socketype_is_inet() {
        let sock = SocketType::Inet(0i32);
        assert!(sock.is_inet());
    }

    #[test]
    fn test_socketype_is_fifo() {
        let sock = SocketType::Fifo(0i32);
        assert!(sock.is_fifo());
    }

    #[test]
    fn test_socketype_is_mq() {
        let sock = SocketType::Mq(0i32);
        assert!(sock.is_mq());
    }
}

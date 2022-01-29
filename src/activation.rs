use crate::errors::SdError;
use std::convert::TryFrom;
use std::env;
use std::mem::MaybeUninit;
use std::os::unix::io::{IntoRawFd, RawFd};
use std::process;

use crate::sys::socket::get_socket_family;
use crate::sys::stdio::fstat;

/// Minimum FD number used by systemd for passing sockets.
const SD_LISTEN_FDS_START: RawFd = 3;

/// Trait for checking the type of a file descriptor.
pub trait IsType {
    /// Returns true if a file descriptor is a FIFO.
    fn is_fifo(&self) -> bool;

    /// Returns true if a file descriptor is a special file.
    fn is_special(&self) -> bool;

    /// Returns true if a file descriptor is a `PF_INET` socket.
    fn is_inet(&self) -> bool;

    /// Returns true if a file descriptor is a `PF_UNIX` socket.
    fn is_unix(&self) -> bool;

    /// Returns true if a file descriptor is a POSIX message queue descriptor.
    fn is_mq(&self) -> bool;
}

/// File descriptor passed by systemd to socket-activated services.
///
/// See <https://www.freedesktop.org/software/systemd/man/systemd.socket.html>.
#[derive(Debug, Clone)]
pub struct FileDescriptor(SocketFd);

/// Possible types of sockets.
#[derive(Debug, Clone)]
enum SocketFd {
    /// A FIFO named pipe (see `man 7 fifo`)
    Fifo(RawFd),
    /// A special file, such as character device nodes or special files in
    /// `/proc` and `/sys`.
    Special(RawFd),
    /// A `PF_INET` socket, such as UDP/TCP sockets.
    Inet(RawFd),
    /// A `PF_UNIX` socket (see `man 7 unix`).
    Unix(RawFd),
    /// A POSIX message queue (see `man 7 mq_overview`).
    Mq(RawFd),
    /// An unknown descriptor (possibly invalid, use with caution).
    Unknown(RawFd),
}

impl IsType for FileDescriptor {
    fn is_fifo(&self) -> bool {
        matches!(self.0, SocketFd::Fifo(_))
    }

    fn is_special(&self) -> bool {
        matches!(self.0, SocketFd::Special(_))
    }

    fn is_unix(&self) -> bool {
        matches!(self.0, SocketFd::Unix(_))
    }

    fn is_inet(&self) -> bool {
        matches!(self.0, SocketFd::Inet(_))
    }

    fn is_mq(&self) -> bool {
        matches!(self.0, SocketFd::Mq(_))
    }
}

/// Check for file descriptors passed by systemd.
///
/// Invoked by socket activated daemons to check for file descriptors needed by the service.
/// If `unset_env` is true, the environment variables used by systemd will be cleared.
pub fn receive_descriptors(unset_env: bool) -> Result<Vec<FileDescriptor>, SdError> {
    let pid = env::var("LISTEN_PID");
    let fds = env::var("LISTEN_FDS");
    log::trace!("LISTEN_PID = {:?}; LISTEN_FDS = {:?}", pid, fds);

    if unset_env {
        env::remove_var("LISTEN_PID");
        env::remove_var("LISTEN_FDS");
        env::remove_var("LISTEN_FDNAMES");
    }

    let pid = pid
        .map_err(|e| format!("failed to get LISTEN_PID: {}", e))?
        .parse::<u32>()
        .map_err(|e| format!("failed to parse LISTEN_PID: {}", e))?;
    let fds = fds
        .map_err(|e| format!("failed to get LISTEN_FDS: {}", e))?
        .parse::<usize>()
        .map_err(|e| format!("failed to parse LISTEN_FDS: {}", e))?;

    if process::id() != pid {
        return Err("PID mismatch".into());
    }

    socks_from_fds(fds)
}

/// Check for named file descriptors passed by systemd.
///
/// Like `receive_descriptors`, but this will also return a vector of names
/// associated with each file descriptor.
pub fn receive_descriptors_with_names(
    unset_env: bool,
) -> Result<Vec<(FileDescriptor, String)>, SdError> {
    let pid = env::var("LISTEN_PID");
    let fds = env::var("LISTEN_FDS");
    let fdnames = env::var("LISTEN_FDNAMES");
    log::trace!(
        "LISTEN_PID = {:?}; LISTEN_FDS = {:?}; LISTEN_FDNAMES = {:?}",
        pid,
        fds,
        fdnames
    );

    if unset_env {
        env::remove_var("LISTEN_PID");
        env::remove_var("LISTEN_FDS");
        env::remove_var("LISTEN_FDNAMES");
    }

    let pid = pid
        .map_err(|e| format!("failed to get LISTEN_PID: {}", e))?
        .parse::<u32>()
        .map_err(|e| format!("failed to parse LISTEN_PID: {}", e))?;
    let fds = fds
        .map_err(|e| format!("failed to get LISTEN_FDS: {}", e))?
        .parse::<usize>()
        .map_err(|e| format!("failed to parse LISTEN_FDS: {}", e))?;

    if process::id() != pid {
        return Err("PID mismatch".into());
    }

    let fdnames = fdnames.map_err(|e| format!("failed to get LISTEN_FDNAMES: {}", e))?;
    let names = fdnames.split(':').map(String::from);
    let vec = socks_from_fds(fds)?;
    let out = vec.into_iter().zip(names).collect();

    Ok(out)
}

fn socks_from_fds(num_fds: usize) -> Result<Vec<FileDescriptor>, SdError> {
    let mut descriptors = Vec::with_capacity(num_fds);
    for fd_offset in 0..num_fds {
        let index = SD_LISTEN_FDS_START
            .checked_add(fd_offset as i32)
            .ok_or_else(|| format!("overlarge file descriptor index: {}", num_fds))?;
        let fd = FileDescriptor::try_from(index).unwrap_or_else(|(msg, val)| {
            log::warn!("{}", msg);
            FileDescriptor(SocketFd::Unknown(val))
        });
        descriptors.push(fd);
    }

    Ok(descriptors)
}

impl IsType for RawFd {
    fn is_fifo(&self) -> bool {
        fstat(*self).map_or(false, |s| (s.st_mode & 0o0_170_000) == 0o010_000)
    }

    fn is_special(&self) -> bool {
        fstat(*self).map_or(false, |s| (s.st_mode & 0o0_170_000) == 0o100_000)
    }

    fn is_inet(&self) -> bool {
        get_socket_family(*self).map_or(false, |f| libc::c_int::from(f) == libc::AF_INET)
    }

    fn is_unix(&self) -> bool {
        get_socket_family(*self).map_or(false, |f| libc::c_int::from(f) == libc::AF_INET)
    }

    fn is_mq(&self) -> bool {
        // SAFETY: We need to pass some pointer to mq_getattr, but we never actually use it.
        unsafe {
            let mut attr = MaybeUninit::zeroed();
            libc::mq_getattr(*self, attr.as_mut_ptr()) == 0
        }
    }
}

impl TryFrom<RawFd> for FileDescriptor {
    type Error = (SdError, RawFd);

    fn try_from(value: RawFd) -> Result<Self, Self::Error> {
        if value.is_fifo() {
            return Ok(FileDescriptor(SocketFd::Fifo(value)));
        } else if value.is_special() {
            return Ok(FileDescriptor(SocketFd::Special(value)));
        } else if value.is_inet() {
            return Ok(FileDescriptor(SocketFd::Inet(value)));
        } else if value.is_unix() {
            return Ok(FileDescriptor(SocketFd::Unix(value)));
        } else if value.is_mq() {
            return Ok(FileDescriptor(SocketFd::Mq(value)));
        }

        let err_msg = format!(
            "conversion failure, possibly invalid or unknown file descriptor {}",
            value
        );
        Err((err_msg.into(), value))
    }
}

// TODO(lucab): replace with multiple safe `TryInto` helpers plus an `unsafe` fallback.
impl IntoRawFd for FileDescriptor {
    fn into_raw_fd(self) -> RawFd {
        match self.0 {
            SocketFd::Fifo(fd) => fd,
            SocketFd::Special(fd) => fd,
            SocketFd::Inet(fd) => fd,
            SocketFd::Unix(fd) => fd,
            SocketFd::Mq(fd) => fd,
            SocketFd::Unknown(fd) => fd,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socketype_is_unix() {
        let sock = FileDescriptor(SocketFd::Unix(0i32));
        assert!(sock.is_unix());
    }

    #[test]
    fn test_socketype_is_special() {
        let sock = FileDescriptor(SocketFd::Special(0i32));
        assert!(sock.is_special());
    }

    #[test]
    fn test_socketype_is_inet() {
        let sock = FileDescriptor(SocketFd::Inet(0i32));
        assert!(sock.is_inet());
    }

    #[test]
    fn test_socketype_is_fifo() {
        let sock = FileDescriptor(SocketFd::Fifo(0i32));
        assert!(sock.is_fifo());
    }

    #[test]
    fn test_socketype_is_mq() {
        let sock = FileDescriptor(SocketFd::Mq(0i32));
        assert!(sock.is_mq());
    }
}

use crate::errors::SdError;
use nix::mqueue::mq_getattr;
use nix::sys::socket::getsockname;
use nix::sys::socket::SockAddr;
use nix::sys::stat::fstat;
use std::convert::TryFrom;
use std::env;
use std::os::unix::io::{IntoRawFd, RawFd};
use std::process;

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
/// See https://www.freedesktop.org/software/systemd/man/systemd.socket.html.
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
}

impl IsType for FileDescriptor {
    fn is_fifo(&self) -> bool {
        match self.0 {
            SocketFd::Fifo(_) => true,
            _ => false,
        }
    }

    fn is_special(&self) -> bool {
        match self.0 {
            SocketFd::Special(_) => true,
            _ => false,
        }
    }

    fn is_unix(&self) -> bool {
        match self.0 {
            SocketFd::Unix(_) => true,
            _ => false,
        }
    }

    fn is_inet(&self) -> bool {
        match self.0 {
            SocketFd::Inet(_) => true,
            _ => false,
        }
    }

    fn is_mq(&self) -> bool {
        match self.0 {
            SocketFd::Mq(_) => true,
            _ => false,
        }
    }
}

/// Check for file descriptors passed by systemd.
///
/// Invoked by socket activated daemons to check for file descriptors needed by the service.
/// If `unset_env` is true, the environment variables used by systemd will be cleared.
pub fn receive_descriptors(unset_env: bool) -> Result<Vec<FileDescriptor>, SdError> {
    let pid = env::var("LISTEN_PID");
    let fds = env::var("LISTEN_FDS");
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
/// Like `sd_listen_fds`, but this will also return a vector of names associated with each file
/// descriptor.
pub fn receive_descriptors_with_names(
    unset_env: bool,
) -> Result<Vec<(FileDescriptor, String)>, SdError> {
    let pid = env::var("LISTEN_PID");
    let fds = env::var("LISTEN_FDS");
    let names = env::var("LISTEN_FDNAMES");

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

    let names: Vec<String> = names
        .map_err(|e| format!("failed to get LISTEN_FDNAMES: {}", e))?
        .split(':')
        .map(String::from)
        .collect();
    let vec = socks_from_fds(fds)?;
    let out = vec.into_iter().zip(names.into_iter()).collect();

    Ok(out)
}

fn socks_from_fds(num_fds: usize) -> Result<Vec<FileDescriptor>, SdError> {
    let mut descriptors = Vec::with_capacity(num_fds);
    for fd_offset in 0..num_fds {
        let fd = SD_LISTEN_FDS_START + (fd_offset as i32);
        let sock = FileDescriptor::try_from(fd)
            .map_err(|e| format!("failed to receive file descriptor {}: {}", fd_offset, e))?;
        descriptors.push(sock);
    }

    Ok(descriptors)
}

impl IsType for RawFd {
    fn is_fifo(&self) -> bool {
        match fstat(*self) {
            Ok(stat) => (stat.st_mode & 0o0_170_000) == 0o010_000,
            Err(_) => false,
        }
    }

    fn is_special(&self) -> bool {
        match fstat(*self) {
            Ok(stat) => (stat.st_mode & 0o0_170_000) == 0o100_000,
            Err(_) => false,
        }
    }

    fn is_inet(&self) -> bool {
        match getsockname(*self) {
            Ok(addr) => {
                if let SockAddr::Inet(_) = addr {
                    true
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    fn is_unix(&self) -> bool {
        match getsockname(*self) {
            Ok(addr) => {
                if let SockAddr::Unix(_) = addr {
                    true
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    fn is_mq(&self) -> bool {
        mq_getattr(*self).is_ok()
    }
}

impl TryFrom<RawFd> for FileDescriptor {
    type Error = SdError;

    fn try_from(value: RawFd) -> Result<Self, SdError> {
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

        Err("invalid file descriptor".into())
    }
}

impl IntoRawFd for FileDescriptor {
    fn into_raw_fd(self) -> RawFd {
        match self.0 {
            SocketFd::Fifo(fd) => fd,
            SocketFd::Special(fd) => fd,
            SocketFd::Inet(fd) => fd,
            SocketFd::Unix(fd) => fd,
            SocketFd::Mq(fd) => fd,
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

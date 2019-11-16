use nix::mqueue::mq_getattr;
use nix::sys::socket::getsockname;
use nix::sys::socket::SockAddr;
use nix::sys::stat::fstat;
use std::convert::TryFrom;
use std::os::unix::io::RawFd;
use std::process;
use std::env;

use errors::*;

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
pub enum FileDescriptor {
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

impl IsType for FileDescriptor {
    fn is_fifo(&self) -> bool {
        match self {
            FileDescriptor::Fifo(_) => true,
            _ => false,
        }
    }

    fn is_special(&self) -> bool {
        match self {
            FileDescriptor::Special(_) => true,
            _ => false,
        }
    }

    fn is_unix(&self) -> bool {
        match self {
            FileDescriptor::Unix(_) => true,
            _ => false,
        }
    }

    fn is_inet(&self) -> bool {
        match self {
            FileDescriptor::Inet(_) => true,
            _ => false,
        }
    }

    fn is_mq(&self) -> bool {
        match self {
            FileDescriptor::Mq(_) => true,
            _ => false,
        }
    }
}

/// Check for file descriptors passed by systemd
///
/// Invoked by socket activated daemons to check for file descriptors needed by the service.
/// If unset_env is true, the environment variables used by systemd will be cleared.
pub fn recieve_descriptors(unset_env: bool) -> Result<Vec<FileDescriptor>> {
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
pub fn recieve_descriptors_with_names(unset_env: bool) -> Result<Vec<(FileDescriptor, String)>> {
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

    let names: Vec<String> = names.split(':').map(String::from).collect();
    let vec = socks_from_fds(fds);
    let out = vec.into_iter().zip(names.into_iter()).collect();

    Ok(out)
}

fn socks_from_fds(num_fds: i32) -> Vec<FileDescriptor> {
    let mut vec = Vec::new();
    for fd in SD_LISTEN_FDS_START..SD_LISTEN_FDS_START+num_fds {
        if let Ok(sock) = FileDescriptor::try_from(fd) {
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
            },
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
            },
            Err(_) => false,
        }
    }

    fn is_mq(&self) -> bool {
        mq_getattr(*self).is_ok()
    }
}

impl TryFrom<RawFd> for FileDescriptor {
    type Error = &'static str;

    fn try_from(value: RawFd) -> std::result::Result<Self, Self::Error> {
        if value.is_fifo() {
            return Ok(FileDescriptor::Fifo(value));
        } else if value.is_special() {
            return Ok(FileDescriptor::Special(value));
        } else if value.is_inet() {
            return Ok(FileDescriptor::Inet(value));
        } else if value.is_unix() {
            return Ok(FileDescriptor::Unix(value));
        } else if value.is_mq() {
            return Ok(FileDescriptor::Mq(value));
        }

        Err("Invalid FD")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socketype_is_unix() {
        let sock = FileDescriptor::Unix(0i32);
        assert!(sock.is_unix());
    }

    #[test]
    fn test_socketype_is_special() {
        let sock = FileDescriptor::Special(0i32);
        assert!(sock.is_special());
    }

    #[test]
    fn test_socketype_is_inet() {
        let sock = FileDescriptor::Inet(0i32);
        assert!(sock.is_inet());
    }

    #[test]
    fn test_socketype_is_fifo() {
        let sock = FileDescriptor::Fifo(0i32);
        assert!(sock.is_fifo());
    }

    #[test]
    fn test_socketype_is_mq() {
        let sock = FileDescriptor::Mq(0i32);
        assert!(sock.is_mq());
    }
}

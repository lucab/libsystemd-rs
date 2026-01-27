use crate::errors::{Context as _, SdError};
use crate::libc::fcntl;
use nix::fcntl::{F_SETFD, FdFlag};
use nix::sys::socket::{AddressFamily, SockaddrLike as _, SockaddrStorage, getsockname};
use nix::sys::stat::fstat;
use std::convert::TryFrom;
use std::env;
use std::mem::MaybeUninit;
use std::os::fd::BorrowedFd;
use std::os::unix::io::{IntoRawFd, RawFd};
use std::process;

/// Minimum FD number used by systemd for passing sockets.
const SD_LISTEN_FDS_START: RawFd = 3;

/// Trait for checking the type of a file descriptor.
pub trait IsType {
    /// Returns true if a file descriptor is a FIFO.
    #[must_use]
    fn is_fifo(&self) -> bool;

    /// Returns true if a file descriptor is a special file.
    #[must_use]
    fn is_special(&self) -> bool;

    /// Returns true if a file descriptor is a `PF_INET` socket.
    #[must_use]
    fn is_inet(&self) -> bool;

    /// Returns true if a file descriptor is a `PF_UNIX` socket.
    #[must_use]
    fn is_unix(&self) -> bool;

    /// Returns true if a file descriptor is a POSIX message queue descriptor.
    #[must_use]
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
    /// A FIFO named pipe (see `man 7 fifo`).
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
    #[inline]
    fn is_fifo(&self) -> bool {
        matches!(self.0, SocketFd::Fifo(_))
    }

    #[inline]
    fn is_special(&self) -> bool {
        matches!(self.0, SocketFd::Special(_))
    }

    #[inline]
    fn is_unix(&self) -> bool {
        matches!(self.0, SocketFd::Unix(_))
    }

    #[inline]
    fn is_inet(&self) -> bool {
        matches!(self.0, SocketFd::Inet(_))
    }

    #[inline]
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
    log::trace!("LISTEN_PID = {pid:?}; LISTEN_FDS = {fds:?}");

    if unset_env {
        unsafe { env::remove_var("LISTEN_PID") };
        unsafe { env::remove_var("LISTEN_FDS") };
        unsafe { env::remove_var("LISTEN_FDNAMES") };
    }

    // Parse `$LISTEN_PID` if present.
    if pid == Err(env::VarError::NotPresent) {
        return Ok(vec![]);
    }
    let pid = pid
        .context("failed to get LISTEN_PID")?
        .parse::<u32>()
        .context("failed to parse LISTEN_PID")?;
    let current_pid = process::id();
    if pid != current_pid {
        log::info!(
            "Ignoring systemd activation settings ($LISTEN_PID={pid}), not meant for current process (PID {current_pid}).",
        );
        return Ok(vec![]);
    }

    // Parse `$LISTEN_FDS` if present.
    if fds == Err(env::VarError::NotPresent) {
        return Ok(vec![]);
    }
    let fds = fds
        .context("failed to get LISTEN_FDS")?
        .parse::<usize>()
        .context("failed to parse LISTEN_FDS")?;

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
    log::trace!("LISTEN_PID = {pid:?}; LISTEN_FDS = {fds:?}; LISTEN_FDNAMES = {fdnames:?}");

    if unset_env {
        unsafe { env::remove_var("LISTEN_PID") };
        unsafe { env::remove_var("LISTEN_FDS") };
        unsafe { env::remove_var("LISTEN_FDNAMES") };
    }

    // Parse `$LISTEN_PID` if present.
    if pid == Err(env::VarError::NotPresent) {
        return Ok(vec![]);
    }
    let pid = pid
        .context("failed to get LISTEN_PID")?
        .parse::<u32>()
        .context("failed to parse LISTEN_PID")?;
    let current_pid = process::id();
    if pid != current_pid {
        log::info!(
            "Ignoring systemd activation settings ($LISTEN_PID={pid}), not meant for current process (PID {current_pid})."
        );
        return Ok(vec![]);
    }

    // Parse `$LISTEN_FDS` if present.
    if fds == Err(env::VarError::NotPresent) {
        return Ok(vec![]);
    }
    let fds = fds
        .context("failed to get LISTEN_FDS")?
        .parse::<usize>()
        .context("failed to parse LISTEN_FDS")?;

    // Parse `$LISTEN_FDNAMES` if present.
    if fdnames == Err(env::VarError::NotPresent) {
        return Ok(vec![]);
    }
    let fdnames = fdnames.context("failed to get LISTEN_FDNAMES")?;
    let names = fdnames.split(':').map(String::from);

    let vec = socks_from_fds(fds).context("failed to get sockets from file descriptor")?;
    let out = vec.into_iter().zip(names).collect();

    Ok(out)
}

fn socks_from_fds(fd_count: usize) -> Result<Vec<FileDescriptor>, SdError> {
    let raw_fd_count = RawFd::try_from(fd_count)
        .with_context(|| format!("overlarge file descriptor index: {fd_count}"))?;

    let mut descriptors = Vec::with_capacity(fd_count);
    for fd_offset in 0i32..raw_fd_count {
        let fd_num = SD_LISTEN_FDS_START
            .checked_add(fd_offset)
            .with_context(|| format!("overlarge file descriptor index: {raw_fd_count}"))?;
        // Set CLOEXEC on the file descriptors we receive so that they aren't
        // passed to programs exec'd from here, just like sd_listen_fds does.
        if let Err(errno) = fcntl(&fd_num, F_SETFD(FdFlag::FD_CLOEXEC)) {
            return Err(format!("couldn't set FD_CLOEXEC on {fd_num}: {errno}").into());
        }
        let fd = FileDescriptor::try_from(fd_num).unwrap_or_else(|(msg, val)| {
            log::warn!("{msg}");
            FileDescriptor(SocketFd::Unknown(val))
        });
        descriptors.push(fd);
    }

    Ok(descriptors)
}

impl IsType for RawFd {
    #[inline]
    fn is_fifo(&self) -> bool {
        if *self == -1i32 {
            return false;
        }
        let raw_fd = unsafe { BorrowedFd::borrow_raw(*self) };
        fstat(raw_fd)
            .map(|stat| (stat.st_mode & 0o0_170_000) == 0o010_000)
            .unwrap_or(false)
    }

    #[inline]
    fn is_special(&self) -> bool {
        if *self == -1i32 {
            return false;
        }
        let raw_fd = unsafe { BorrowedFd::borrow_raw(*self) };
        fstat(raw_fd)
            .map(|stat| (stat.st_mode & 0o0_170_000) == 0o100_000)
            .unwrap_or(false)
    }

    #[inline]
    fn is_inet(&self) -> bool {
        getsockname::<SockaddrStorage>(*self)
            .map(|addr| {
                matches!(
                    addr.family(),
                    Some(AddressFamily::Inet | AddressFamily::Inet6)
                )
            })
            .unwrap_or(false)
    }

    #[inline]
    fn is_unix(&self) -> bool {
        getsockname::<SockaddrStorage>(*self)
            .map(|addr| matches!(addr.family(), Some(AddressFamily::Unix)))
            .unwrap_or(false)
    }

    #[inline]
    fn is_mq(&self) -> bool {
        // `nix` does not enable us to test if a raw fd is a mq, so we must drop to libc here.
        // SAFETY: `mq_getattr` is specified to return -1 when passed a fd which is not a mq.
        //         Furthermore, we ignore `attr` and rely only on the return value.
        let mut attr = MaybeUninit::<libc::mq_attr>::uninit();
        let res = unsafe { libc::mq_getattr(*self, attr.as_mut_ptr()) };
        res == 0
    }
}

impl TryFrom<RawFd> for FileDescriptor {
    type Error = (SdError, RawFd);

    #[inline]
    fn try_from(value: RawFd) -> Result<Self, Self::Error> {
        if value.is_fifo() {
            Ok(Self(SocketFd::Fifo(value)))
        } else if value.is_special() {
            Ok(Self(SocketFd::Special(value)))
        } else if value.is_inet() {
            Ok(Self(SocketFd::Inet(value)))
        } else if value.is_unix() {
            Ok(Self(SocketFd::Unix(value)))
        } else if value.is_mq() {
            Ok(Self(SocketFd::Mq(value)))
        } else {
            let err_msg =
                format!("conversion failure, possibly invalid or unknown file descriptor {value}");
            Err((err_msg.into(), value))
        }
    }
}

// TODO(lucab): replace with multiple safe `TryInto` helpers plus an `unsafe` fallback.
impl IntoRawFd for FileDescriptor {
    #[inline]
    fn into_raw_fd(self) -> RawFd {
        match self.0 {
            SocketFd::Fifo(fd)
            | SocketFd::Special(fd)
            | SocketFd::Inet(fd)
            | SocketFd::Unix(fd)
            | SocketFd::Mq(fd)
            | SocketFd::Unknown(fd) => fd,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::activation::{FileDescriptor, IsType as _, SocketFd};

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

use libc::c_int;
use std::io::{Error, Result};
use std::mem::MaybeUninit;
use std::os::unix::io::RawFd;

/// Seal a file descriptor.
pub fn seal(fd: RawFd, seals: c_int) -> Result<()> {
    unsafe {
        if libc::fcntl(fd, libc::F_ADD_SEALS, seals) < 0 {
            Err(Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

/// Add all seals to a file descriptor
pub fn seal_all(fd: RawFd) -> Result<()> {
    seal(
        fd,
        libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE | libc::F_SEAL_SEAL,
    )
}

/// Stat a file descriptor.
pub fn fstat(fd: RawFd) -> Result<libc::stat> {
    unsafe {
        let mut stat = MaybeUninit::zeroed();
        if libc::fstat(fd, stat.as_mut_ptr()) == 0 {
            Ok(stat.assume_init())
        } else {
            Err(Error::last_os_error())
        }
    }
}

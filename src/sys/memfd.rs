use std::fs::File;
use std::io::{Error, Result};
use std::os::unix::prelude::*;

use libc::c_uint;

/// Create a new memfd with the given flags.
pub fn create(flags: c_uint) -> Result<File> {
    // SAFETY: Take ownership of the memfd if it was returned successfully, otherwise we fail.
    // We also explicitly add the trailing null byte to terminate the name.
    unsafe {
        let fd = libc::memfd_create(
            "libsystemd-rs-logging\0".as_ptr() as *const libc::c_char,
            flags,
        );
        if fd < 0 {
            Err(Error::last_os_error())
        } else {
            Ok(File::from_raw_fd(fd))
        }
    }
}

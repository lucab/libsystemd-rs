use std::io::{Error, Result};
use std::mem::{size_of, MaybeUninit};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixDatagram;
use std::os::unix::prelude::{AsRawFd, RawFd};
use std::path::Path;
use std::ptr;

use libc::*;

pub fn get_socket_family(fd: RawFd) -> Result<libc::sa_family_t> {
    // SAFETY: getsockname initializes storage on success, otherwise we discard it
    unsafe {
        let mut storage = MaybeUninit::zeroed();
        let mut size = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if libc::getsockname(fd, storage.as_mut_ptr(), &mut size) == 0 {
            Ok(storage.assume_init().sa_family)
        } else {
            Err(Error::last_os_error())
        }
    }
}

const CMSG_BUFSIZE: usize = 64;

/// Internal unions which lets use create arbitrary buffers
/// with proper alignment for cmsghdr structs.
#[repr(C)]
union AlignedBuffer<T: Copy + Clone> {
    buffer: T,
    align: cmsghdr,
}

fn assert_cmsg_bufsize() {
    let space_one_fd = unsafe { CMSG_SPACE(size_of::<RawFd>() as u32) };
    assert!(
        space_one_fd <= CMSG_BUFSIZE as u32,
        "cmsghdr buffer too small (< {}) to hold a single fd",
        space_one_fd
    );
}

#[cfg(test)]
#[test]
fn cmsg_buffer_size_for_one_fd() {
    assert_cmsg_bufsize()
}

pub fn send_one_fd_to<P: AsRef<Path>>(socket: &UnixDatagram, fd: RawFd, path: P) -> Result<usize> {
    assert_cmsg_bufsize();

    // SAFETY: 0 is a valid value for every type in sockaddr_un, so we're not invoking UB.
    // However we cannot initialize sockaddr_un directly because some architectures may have
    // private padding fields.
    let mut addr: sockaddr_un = unsafe { std::mem::zeroed() };
    let path_bytes = path.as_ref().as_os_str().as_bytes();
    // path_bytes may have at most sun_path + 1 bytes, to account for the trailing NUL byte.
    if addr.sun_path.len() <= path_bytes.len() {
        return Err(Error::from_raw_os_error(ENAMETOOLONG));
    }

    addr.sun_family = AF_UNIX as _;
    // SAFETY: We initialized path_bytes with the value of path and checked that its length
    // does not exceed that of addr.sun_path.  We do not need to add the trailing NULL byte
    // explicitly, because we explicitly initialized addr and thus addr.sun_path with 0.
    unsafe {
        std::ptr::copy_nonoverlapping(
            path_bytes.as_ptr(),
            addr.sun_path.as_mut_ptr() as *mut u8,
            path_bytes.len(),
        )
    };

    // SAFETY: 0 is a valid value for every type in msghdr, so we're not invoking UB.
    // But again we cannot initialize msghdr because of private padding fields.
    let mut msg: msghdr = unsafe { std::mem::zeroed() };
    // Set the target address.
    msg.msg_name = &mut addr as *mut _ as *mut c_void;
    msg.msg_namelen = size_of::<sockaddr_un>() as socklen_t;

    // We send no data body with this message.
    msg.msg_iov = ptr::null_mut();
    msg.msg_iovlen = 0;

    // Create and fill the control message buffer with our file descriptor
    let mut cmsg_buffer = AlignedBuffer {
        buffer: ([0u8; CMSG_BUFSIZE]),
    };
    // SAFETY: We just created cmsg_buffer, so its ours to pass on, and we explicitly
    // tell C abouts its size with proper padding (by means of CMSG_SPACE).  Thanks to
    // our AlignedBuffer union our buffer also has proper alignment for the msg_control
    // field.
    msg.msg_control = unsafe { cmsg_buffer.buffer.as_mut_ptr() as _ };
    msg.msg_controllen = unsafe { CMSG_SPACE(size_of::<RawFd>() as _) as _ };

    // SAFETY: We just set the msg.msg_control pointer to a proper buffer and made sure
    // that C knows about its size, so we can now safely get hold of the first control
    // message header of the socket message.  This header will be somewhere in our previously
    // allocated cmsg_buffer.
    let mut cmsg: &mut cmsghdr =
        unsafe { CMSG_FIRSTHDR(&msg).as_mut() }.expect("Control message buffer exhausted");

    cmsg.cmsg_level = SOL_SOCKET;
    cmsg.cmsg_type = SCM_RIGHTS;
    // SAFETY: CMSG_LEN gives us the appropriate size for a message which holds just a single
    // file descriptor.
    cmsg.cmsg_len = unsafe { CMSG_LEN(size_of::<RawFd>() as _) as _ };

    unsafe { ptr::write(CMSG_DATA(cmsg) as *mut RawFd, fd) };

    let result = unsafe { sendmsg(socket.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) };

    if result < 0 {
        Err(Error::last_os_error())
    } else {
        // sendmsg returns the number of bytes written
        Ok(result as usize)
    }
}

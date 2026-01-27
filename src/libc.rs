use libc::c_int;
use nix::Result;
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, fcntl as nix_fcntl};
use std::os::fd::{AsRawFd, BorrowedFd};

pub fn fcntl<Fd: AsRawFd>(fd: &Fd, arg: FcntlArg) -> Result<c_int> {
    let fd = fd.as_raw_fd();
    if fd == -1i32 {
        return Err(Errno::EINVAL);
    }
    let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
    nix_fcntl(borrowed_fd, arg)
}

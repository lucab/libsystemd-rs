use std::fs::File;
use std::io::Write;
use std::os::unix::io::FromRawFd;
use std::ffi::CString;
use std::os::unix::net::UnixDatagram;
use std::os::unix::io::{AsRawFd, IntoRawFd};
use nix::sys::socket::{ControlMessage, MsgFlags, sendmsg, SockAddr};
use nix::sys::memfd::MemFdCreateFlag;
use nix::sys::memfd::memfd_create;
use nix::fcntl::*;

use crate::errors::*;

/// Default path of the journald AF_UNIX datagram socket.
const SD_JOURNAL_SOCK_PATH: &str = "/run/systemd/journal/socket";

/// Log priority values.
///
/// See `man 3 syslog`.
#[derive(Debug)]
#[repr(u8)]
pub enum Priority {
    /// system is unusable
    Emergency = 0,
    /// action must be taken immediately
    Alert,
    /// critical conditions
    Critical,
    /// error conditions
    Error,
    /// warning conditions
    Warning,
    /// normal, but significant, condition
    Notice,
    /// informational message
    Info,
    /// debug-level message
    Debug,
}

impl std::convert::From<Priority> for u8 {
    fn from(p: Priority) -> Self {
        match p {
            Priority::Emergency => 0,
            Priority::Alert => 1,
            Priority::Critical => 2,
            Priority::Error => 3,
            Priority::Warning => 4,
            Priority::Notice => 5,
            Priority::Info => 6,
            Priority::Debug => 7,
        }
    }
}

#[inline(always)]
fn is_valid_char(c: char) -> bool {
    c.is_uppercase() || c.is_numeric() || c == '_'
}

/// The variable name must be in uppercase and consist only of characters, 
/// numbers and underscores, and may not begin with an underscore.
fn is_valid_field(input: &str) -> bool {
    if input.is_empty() {
        return false;
    }

    if !input.chars().all(is_valid_char) {
        return false;
    }

    if input.starts_with('_') {
        return false;
    }

    true
}

fn add_field_and_payload(data: &mut String, field: &str, payload: &str) {
    if is_valid_field(field) {
        let field_payload = format!("{}={}\n", field, payload);
        data.push_str(&field_payload);
    }
}

/// Print a message to the systemd journal with the given priority.
pub fn journal_print(priority: Priority, msg: &str) -> Result<()> {
    let sock = UnixDatagram::unbound()?;

    let mut data = String::new();
    add_field_and_payload(&mut data, "PRIORITY", &(u8::from(priority)).to_string());
    add_field_and_payload(&mut data, "MESSAGE", msg);

    let res = sock.send_to(data.as_bytes(), SD_JOURNAL_SOCK_PATH);
    match res {
        Ok(_) => return Ok(()),
        // If error code is 90, the message was too long for a UNIX socket.
        Err(ref e) if e.raw_os_error() == Some(90) => {
            let tmpfd = memfd_create(&CString::new("journald")?, MemFdCreateFlag::MFD_ALLOW_SEALING)?;
            // Safe because memfd_create gave us this FD.
            let mut file = unsafe { File::from_raw_fd(tmpfd) };
            file.write_all(data.as_bytes())?;

            let memfd = file.into_raw_fd();
            fcntl(memfd, FcntlArg::F_ADD_SEALS(SealFlag::all()))?;
            let fds = &[memfd];
            let ancillary = [ControlMessage::ScmRights(fds)];
            let path = SockAddr::new_unix(SD_JOURNAL_SOCK_PATH)?;
            sendmsg(sock.as_raw_fd(), &[], &ancillary, MsgFlags::empty(), Some(&path))?;
        },
        _ => return Err("unknown err".into()),
    };

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_print_simple() {
        let res = journal_print(Priority::Info, "TEST LOG!");
        assert!(res.is_ok());
    }

    #[test]
    fn test_journal_print_large_buffer() {
        let data = "A".repeat(212995);
        let res = journal_print(Priority::Debug, &data);
        assert!(res.is_ok());
    }

    #[test]
    fn test_is_valid_field_lowercase_invalid() {
        let field = "test";
        assert_eq!(is_valid_field(&field), false);
    }

    #[test]
    fn test_is_valid_field_uppercase_valid() {
        let field = "TEST";
        assert_eq!(is_valid_field(&field), true);
    }

    #[test]
    fn test_is_valid_field_uppercase_non_alpha_invalid() {
        let field = "TE!ST";
        assert_eq!(is_valid_field(&field), false);
    }

    #[test]
    fn test_is_valid_field_uppercase_leading_underscore_invalid() {
        let field = "_TEST";
        assert_eq!(is_valid_field(&field), false);
    }
}

use nix::fcntl::*;
use nix::sys::memfd::memfd_create;
use nix::sys::memfd::MemFdCreateFlag;
use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags, SockAddr};
use std::collections::HashMap;
use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::os::unix::io::FromRawFd;
use std::os::unix::io::{AsRawFd, IntoRawFd};
use std::os::unix::net::UnixDatagram;

use crate::errors::*;

/// Default path of the systemd-journald `AF_UNIX` datagram socket.
pub static SD_JOURNAL_SOCK_PATH: &str = "/run/systemd/journal/socket";

/// Log priority values.
///
/// See `man 3 syslog`.
#[derive(Debug)]
#[repr(u8)]
pub enum Priority {
    /// System is unusable.
    Emergency = 0,
    /// Action must be taken immediately.
    Alert,
    /// Critical condition,
    Critical,
    /// Error condition.
    Error,
    /// Warning condition.
    Warning,
    /// Normal, but significant, condition.
    Notice,
    /// Informational message.
    Info,
    /// Debug message.
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
/// Send a message with structured properties to the journal.
///
/// The PRIORITY or MESSAGE fields from the vars iterator are always ignored in favour of the priority and message arguments.
pub fn journal_send<K, V>(
    priority: Priority,
    msg: &str,
    vars: impl Iterator<Item = (K, V)>,
) -> Result<()>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    let sock = UnixDatagram::unbound()?;

    let mut data = String::new();
    add_field_and_payload(&mut data, "PRIORITY", &(u8::from(priority)).to_string());
    add_field_and_payload(&mut data, "MESSAGE", msg);
    for (ref k, ref v) in vars {
        if k.as_ref() != "PRIORITY" && k.as_ref() != "MESSAGE" {
            add_field_and_payload(&mut data, k.as_ref(), v.as_ref())
        }
    }

    // Message sending logic:
    //  * fast path: data within datagram body.
    //  * slow path: data in a sealed memfd, which is sent as an FD in ancillary data.
    //
    // Maximum data size is system dependent, thus this always tries the fast path and
    // falls back to the slow path if the former fails with `EMSGSIZE`.
    let fast_res = sock.send_to(data.as_bytes(), SD_JOURNAL_SOCK_PATH);
    let res = match fast_res {
        // `EMSGSIZE` (errno code 90) means the message was too long for a UNIX socket,
        Err(ref err) if err.raw_os_error() == Some(90) => send_memfd_payload(sock, data.as_bytes()),
        r => r.map_err(|err| err.into()),
    };

    res.chain_err(|| format!("failed to print to journal at '{}'", SD_JOURNAL_SOCK_PATH))?;
    Ok(())
}
/// Print a message to the journal with the given priority.
pub fn journal_print(priority: Priority, msg: &str) -> Result<()> {
    let map: HashMap<&str, &str> = HashMap::new();
    journal_send(priority, msg, map.iter())
}

/// Send an overlarge payload to systemd-journald socket.
///
/// This is a slow-path for sending a large payload that could not otherwise fit
/// in a UNIX datagram. Payload is thus written to a memfd, which is sent as ancillary
/// data.
fn send_memfd_payload(sock: UnixDatagram, data: &[u8]) -> Result<usize> {
    let memfd = {
        let tmpfd = memfd_create(
            &CString::new("libsystemd-rs-logging")?,
            MemFdCreateFlag::MFD_ALLOW_SEALING,
        )?;
        // SAFETY: `memfd_create` just returned this FD.
        let mut file = unsafe { File::from_raw_fd(tmpfd) };
        file.write_all(data)?;
        file.into_raw_fd()
    };
    // Seal the memfd, so that journald knows it can safely mmap/read it.
    fcntl(memfd, FcntlArg::F_ADD_SEALS(SealFlag::all()))?;

    let fds = &[memfd];
    let ancillary = [ControlMessage::ScmRights(fds)];
    let path = SockAddr::new_unix(SD_JOURNAL_SOCK_PATH)?;
    sendmsg(
        sock.as_raw_fd(),
        &[],
        &ancillary,
        MsgFlags::empty(),
        Some(&path),
    )?;

    Ok(data.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_journald_socket() -> bool {
        match std::fs::metadata(SD_JOURNAL_SOCK_PATH) {
            Ok(_) => true,
            Err(_) => {
                eprintln!(
                    "skipped, journald socket not found at '{}'",
                    SD_JOURNAL_SOCK_PATH
                );
                false
            }
        }
    }

    #[test]
    fn test_journal_print_simple() {
        if !ensure_journald_socket() {
            return;
        }

        journal_print(Priority::Info, "TEST LOG!").unwrap();
    }

    #[test]
    fn test_journal_print_large_buffer() {
        if !ensure_journald_socket() {
            return;
        }

        let data = "A".repeat(212995);
        journal_print(Priority::Debug, &data).unwrap();
    }

    #[test]
    fn test_journal_send_simple() {
        if !ensure_journald_socket() {
            return;
        }

        let mut map: HashMap<&str, &str> = HashMap::new();
        map.insert("TEST_JOURNALD_LOG1", "foo");
        map.insert("TEST_JOURNALD_LOG2", "bar");
        journal_send(Priority::Info, "Test Journald Log", map.iter()).unwrap()
    }
    #[test]
    fn test_journal_skip_fields() {
        if !ensure_journald_socket() {
            return;
        }

        let mut map: HashMap<&str, &str> = HashMap::new();
        let priority = format!("{}", u8::from(Priority::Warning));
        map.insert("TEST_JOURNALD_LOG3", "result");
        map.insert("PRIORITY", &priority);
        map.insert("MESSAGE", "Duplicate value");
        journal_send(Priority::Info, "Test Skip Fields", map.iter()).unwrap()
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

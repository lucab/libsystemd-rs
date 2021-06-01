use crate::errors::SdError;
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

/// Default path of the systemd-journald `AF_UNIX` datagram socket.
pub static SD_JOURNAL_SOCK_PATH: &str = "/run/systemd/journal/socket";

/// Trait for checking the type of a file descriptor.

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

/// Add `field` and `payload` to journal fields `data` with explicit length encoding.
///
/// Write
///
/// 1. the field name,
/// 2. an ASCII newline,
/// 3. the payload size as LE encoded 64 bit integer,
/// 4. the payload, and
/// 5. a final ASCII newline
///
/// to `data`.
///
/// See <https://systemd.io/JOURNAL_NATIVE_PROTOCOL/> for details.
fn add_field_and_payload_explicit_length(data: &mut Vec<u8>, field: &str, payload: &str) {
    data.extend(field.as_bytes());
    data.push(b'\n');
    data.extend(&(payload.len() as u64).to_le_bytes());
    data.extend(payload.as_bytes());
    data.push(b'\n');
}

/// Add  a journal `field` and its `payload` to journal fields `data` with appropriate encoding.
///
/// If `payload` does not contain a newline character use the simple journal field encoding, and
/// write the field name and the payload separated by `=` and suffixed by a final new line.
///
/// Otherwise encode the payload length explicitly with [[`add_field_and_payload_explicit_length`]].
///
/// See <https://systemd.io/JOURNAL_NATIVE_PROTOCOL/> for details.
fn add_field_and_payload(data: &mut Vec<u8>, field: &str, payload: &str) {
    if is_valid_field(field) {
        if payload.contains('\n') {
            add_field_and_payload_explicit_length(data, field, payload);
        } else {
            // If payload doesn't contain an newline directly write the field name and the payload
            data.extend(field.as_bytes());
            data.push(b'=');
            data.extend(payload.as_bytes());
            data.push(b'\n');
        }
    }
}

/// Send a message with structured properties to the journal.
///
/// The PRIORITY or MESSAGE fields from the vars iterator are always ignored in favour of the priority and message arguments.
pub fn journal_send<K, V>(
    priority: Priority,
    msg: &str,
    vars: impl Iterator<Item = (K, V)>,
) -> Result<(), SdError>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    let sock =
        UnixDatagram::unbound().map_err(|e| format!("failed to open datagram socket: {}", e))?;

    let mut data = Vec::new();
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
    let fast_res = sock.send_to(&data, SD_JOURNAL_SOCK_PATH);
    let res = match fast_res {
        // `EMSGSIZE` (errno code 90) means the message was too long for a UNIX socket,
        Err(ref err) if err.raw_os_error() == Some(90) => send_memfd_payload(sock, &data),
        r => r.map_err(|err| err.to_string().into()),
    };

    res.map_err(|e| {
        format!(
            "failed to print to journal at '{}': {}",
            SD_JOURNAL_SOCK_PATH, e
        )
    })?;
    Ok(())
}
/// Print a message to the journal with the given priority.
pub fn journal_print(priority: Priority, msg: &str) -> Result<(), SdError> {
    let map: HashMap<&str, &str> = HashMap::new();
    journal_send(priority, msg, map.iter())
}

/// Send an overlarge payload to systemd-journald socket.
///
/// This is a slow-path for sending a large payload that could not otherwise fit
/// in a UNIX datagram. Payload is thus written to a memfd, which is sent as ancillary
/// data.
fn send_memfd_payload(sock: UnixDatagram, data: &[u8]) -> Result<usize, SdError> {
    let memfd = {
        let fdname = &CString::new("libsystemd-rs-logging").map_err(|e| e.to_string())?;
        let tmpfd =
            memfd_create(fdname, MemFdCreateFlag::MFD_ALLOW_SEALING).map_err(|e| e.to_string())?;

        // SAFETY: `memfd_create` just returned this FD.
        let mut file = unsafe { File::from_raw_fd(tmpfd) };
        file.write_all(data).map_err(|e| e.to_string())?;
        file.into_raw_fd()
    };

    // Seal the memfd, so that journald knows it can safely mmap/read it.
    fcntl(memfd, FcntlArg::F_ADD_SEALS(SealFlag::all())).map_err(|e| e.to_string())?;

    let fds = &[memfd];
    let ancillary = [ControlMessage::ScmRights(fds)];
    let path = SockAddr::new_unix(SD_JOURNAL_SOCK_PATH).map_err(|e| e.to_string())?;
    sendmsg(
        sock.as_raw_fd(),
        &[],
        &ancillary,
        MsgFlags::empty(),
        Some(&path),
    )
    .map_err(|e| e.to_string())?;

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

    #[test]
    fn add_field_and_payload_explicit_length_simple() {
        let mut data = Vec::new();
        add_field_and_payload_explicit_length(&mut data, "FOO", "BAR");
        assert_eq!(
            data,
            vec![b'F', b'O', b'O', b'\n', 3, 0, 0, 0, 0, 0, 0, 0, b'B', b'A', b'R', b'\n']
        );
    }

    #[test]
    fn add_field_and_payload_explicit_length_internal_newline() {
        let mut data = Vec::new();
        add_field_and_payload_explicit_length(&mut data, "FOO", "B\nAR");
        assert_eq!(
            data,
            vec![b'F', b'O', b'O', b'\n', 4, 0, 0, 0, 0, 0, 0, 0, b'B', b'\n', b'A', b'R', b'\n']
        );
    }

    #[test]
    fn add_field_and_payload_explicit_length_trailing_newline() {
        let mut data = Vec::new();
        add_field_and_payload_explicit_length(&mut data, "FOO", "BAR\n");
        assert_eq!(
            data,
            vec![b'F', b'O', b'O', b'\n', 4, 0, 0, 0, 0, 0, 0, 0, b'B', b'A', b'R', b'\n', b'\n']
        );
    }

    #[test]
    fn add_field_and_payload_simple() {
        let mut data = Vec::new();
        add_field_and_payload(&mut data, "FOO", "BAR");
        assert_eq!(data, "FOO=BAR\n".as_bytes());
    }

    #[test]
    fn add_field_and_payload_internal_newline() {
        let mut data = Vec::new();
        add_field_and_payload(&mut data, "FOO", "B\nAR");
        assert_eq!(
            data,
            vec![b'F', b'O', b'O', b'\n', 4, 0, 0, 0, 0, 0, 0, 0, b'B', b'\n', b'A', b'R', b'\n']
        );
    }

    #[test]
    fn add_field_and_payload_trailing_newline() {
        let mut data = Vec::new();
        add_field_and_payload(&mut data, "FOO", "BAR\n");
        assert_eq!(
            data,
            vec![b'F', b'O', b'O', b'\n', 4, 0, 0, 0, 0, 0, 0, 0, b'B', b'A', b'R', b'\n', b'\n']
        );
    }
}

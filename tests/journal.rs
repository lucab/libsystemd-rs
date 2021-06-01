use std::process::Command;

use libsystemd::logging::Priority;
use std::collections::HashMap;
use std::ffi::OsStr;

/// Read from journal.
///
/// `matches` are additional matches to filter journal entries.
fn read_from_journal<I, S>(matches: I) -> Vec<HashMap<String, String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let stdout = String::from_utf8(
        Command::new("journalctl")
            .args(&["--user", "--output=json"])
            // Filter by the PID of the current test process
            .arg(format!("_PID={}", std::process::id()))
            .args(matches)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn simple_message() {
    libsystemd::logging::journal_send(
        Priority::Info,
        "Hello World",
        vec![
            ("TEST_NAME", "simple_message"),
            ("FOO", "another piece of data"),
        ]
        .into_iter(),
    )
    .unwrap();

    let messages = read_from_journal(&["TEST_NAME=simple_message"]);
    assert_eq!(messages.len(), 1);

    let message = &messages[0];
    assert_eq!(message["MESSAGE"], "Hello World");
    assert_eq!(message["TEST_NAME"], "simple_message");
    assert_eq!(message["PRIORITY"], "6");
    assert_eq!(message["FOO"], "another piece of data");
}

#[test]
fn multiline_message() {
    libsystemd::logging::journal_send(
        Priority::Info,
        "Hello\nMultiline\nWorld",
        vec![("TEST_NAME", "multiline_message")].into_iter(),
    )
    .unwrap();

    let messages = read_from_journal(&["TEST_NAME=multiline_message"]);
    assert_eq!(messages.len(), 1);

    let message = &messages[0];
    assert_eq!(message["MESSAGE"], "Hello\nMultiline\nWorld");
    assert_eq!(message["TEST_NAME"], "multiline_message");
    assert_eq!(message["PRIORITY"], "6");
}

#[test]
fn multiline_message_trailing_newline() {
    libsystemd::logging::journal_send(
        Priority::Info,
        "A trailing newline\n",
        vec![("TEST_NAME", "multiline_message_trailing_newline")].into_iter(),
    )
    .unwrap();

    let messages = read_from_journal(&["TEST_NAME=multiline_message_trailing_newline"]);
    assert_eq!(messages.len(), 1);

    let message = &messages[0];
    assert_eq!(message["MESSAGE"], "A trailing newline\n");
    assert_eq!(message["TEST_NAME"], "multiline_message_trailing_newline");
    assert_eq!(message["PRIORITY"], "6");
}

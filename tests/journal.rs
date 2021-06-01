use std::process::Command;

use libsystemd::logging::Priority;
use rand::distributions::Alphanumeric;
use rand::Rng;
use std::collections::HashMap;

fn random_name(prefix: &str) -> String {
    format!(
        "{}_{}",
        prefix,
        rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect::<String>()
    )
}

/// Read from journal with `journalctl`.
///
/// `test_name` is the randomized name of the test being run, and gets
/// added as `TEST_NAME` match to the `journalctl` call, to make sure to
/// only select journal entries originating from and relevant to the
/// current test.
fn read_from_journal(test_name: &str) -> Vec<HashMap<String, String>> {
    let stdout = String::from_utf8(
        Command::new("journalctl")
            .args(&["--user", "--output=json"])
            // Filter by the PID of the current test process
            .arg(format!("_PID={}", std::process::id()))
            .arg(format!("TEST_NAME={}", test_name))
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
    let test_name = random_name("simple_message");
    libsystemd::logging::journal_send(
        Priority::Info,
        "Hello World",
        vec![
            ("TEST_NAME", test_name.as_str()),
            ("FOO", "another piece of data"),
        ]
        .into_iter(),
    )
    .unwrap();

    let messages = read_from_journal(&test_name);
    assert_eq!(messages.len(), 1);

    let message = &messages[0];
    assert_eq!(message["MESSAGE"], "Hello World");
    assert_eq!(message["TEST_NAME"], test_name);
    assert_eq!(message["PRIORITY"], "6");
    assert_eq!(message["FOO"], "another piece of data");
}

#[test]
fn multiline_message() {
    let test_name = random_name("multiline_message");
    libsystemd::logging::journal_send(
        Priority::Info,
        "Hello\nMultiline\nWorld",
        vec![("TEST_NAME", test_name.as_str())].into_iter(),
    )
    .unwrap();

    let messages = read_from_journal(&test_name);
    assert_eq!(messages.len(), 1);

    let message = &messages[0];
    assert_eq!(message["MESSAGE"], "Hello\nMultiline\nWorld");
    assert_eq!(message["TEST_NAME"], test_name);
    assert_eq!(message["PRIORITY"], "6");
}

#[test]
fn multiline_message_trailing_newline() {
    let test_name = random_name("multiline_message_trailing_newline");
    libsystemd::logging::journal_send(
        Priority::Info,
        "A trailing newline\n",
        vec![("TEST_NAME", test_name.as_str())].into_iter(),
    )
    .unwrap();

    let messages = read_from_journal(&test_name);
    assert_eq!(messages.len(), 1);

    let message = &messages[0];
    assert_eq!(message["MESSAGE"], "A trailing newline\n");
    assert_eq!(message["TEST_NAME"], test_name);
    assert_eq!(message["PRIORITY"], "6");
}

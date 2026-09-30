//! Exit code tests for `mixxx-now-playing command` (ADR 0007 §The Command
//! Line).
//!
//! Each test gives a command line that is not correct. The binary stops before
//! it looks for a device. No test opens a device, because this computer can
//! have a real V4V card and a running Mixxx.

use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mixxx-now-playing"))
}

/// Runs the binary with `args` and checks exit code 2 and one usage line.
fn assert_usage_error(args: &[&str]) {
    let output = Command::new(binary())
        .args(args)
        .output()
        .expect("run mixxx-now-playing");

    assert_eq!(output.status.code(), Some(2), "{args:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.lines().count(), 1, "{args:?}: {stderr:?}");
    assert!(
        stderr.contains("usage: mixxx-now-playing command fade-now"),
        "{args:?}: {stderr:?}"
    );
    assert!(output.stdout.is_empty(), "{args:?}");
}

#[test]
fn a_missing_command_name_gives_exit_code_2() {
    assert_usage_error(&["command"]);
}

#[test]
fn an_unknown_command_name_gives_exit_code_2() {
    assert_usage_error(&["command", "skip-next"]);
    assert_usage_error(&["command", "--timeout", "1"]);
}

#[test]
fn a_timeout_that_is_not_a_positive_number_gives_exit_code_2() {
    for timeout in ["0", "-1", "abc", "", "inf", "NaN"] {
        assert_usage_error(&["command", "fade-now", "--timeout", timeout]);
    }
    assert_usage_error(&["command", "fade-now", "--timeout"]);
}

#[test]
fn an_unknown_option_gives_exit_code_2() {
    assert_usage_error(&["command", "fade-now", "--verbose"]);
    assert_usage_error(&["command", "fade-now", "--connector-card"]);
}

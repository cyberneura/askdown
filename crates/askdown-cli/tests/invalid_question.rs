//! A question block that cannot be read must stop askdown before any window
//! opens, so that the agent that wrote the file sees the mistake.
//!
//! `--timeout` is there for a regression: if a window did open, the test fails on
//! the exit code instead of waiting for someone to close it.

use std::path::PathBuf;
use std::process::{Command, Output};

const BROKEN: &str =
    "# Title\n\n```askdown\ntype: text\n```\n\n```askdown\ntype: text\nquestion: a: b\n```\n";

fn scratch(test: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "askdown-invalid-question-{}-{test}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("questions.md");
    std::fs::write(&path, content).unwrap();
    path
}

fn askdown(args: &[&str], path: &PathBuf) -> Output {
    Command::new(env!("CARGO_BIN_EXE_askdown"))
        .args(args)
        .arg(path)
        .output()
        .unwrap()
}

#[test]
fn a_broken_block_exits_1_with_the_error_on_stderr_and_touches_nothing() {
    let path = scratch("run", BROKEN);
    let output = askdown(&["--timeout", "1"], &path);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(output.stdout.is_empty());
    assert!(
        stderr.contains("question 2 (opening fence on line 7;"),
        "{stderr}"
    );
    assert!(stderr.contains("mapping values"), "{stderr}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), BROKEN);
}

#[test]
fn list_still_prints_the_questions_but_exits_1() {
    let path = scratch("list", BROKEN);
    let output = askdown(&["--list"], &path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("[1] q1: Text"), "{stdout}");
    assert!(stdout.contains("[2] q2: invalid:"), "{stdout}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("question 2"));
}

#[test]
fn list_of_readable_questions_exits_0() {
    let path = scratch("readable", "```askdown\ntype: text\n```\n");
    assert_eq!(askdown(&["--list"], &path).status.code(), Some(0));
}

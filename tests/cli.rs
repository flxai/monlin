use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

#[test]
fn help_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .arg("--help")
        .output()
        .expect("failed to run monlin --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: monlin [OPTIONS] [LAYOUT]..."));
}

#[test]
fn zsh_completion_can_be_printed() {
    let output = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args(["completion", "zsh"])
        .output()
        .expect("failed to run monlin completion zsh");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("#compdef monlin"));
    assert!(stdout.contains("_monlin_layout"));
    assert!(stdout.contains("pct hum free"));
    assert!(stdout.contains("--space:How streamed columns allocate width"));
}

#[test]
fn other_shell_completions_can_be_printed() {
    for shell in ["bash", "fish", "elvish", "power-shell"] {
        let output = Command::new(env!("CARGO_BIN_EXE_monlin"))
            .args(["completion", shell])
            .output()
            .unwrap_or_else(|_| panic!("failed to run monlin completion {shell}"));

        assert!(output.status.success(), "completion failed for {shell}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.trim().is_empty(), "empty completion for {shell}");
    }
}

#[test]
fn debug_colors_command_prints_metric_rows() {
    let output = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args(["--color", "never", "debug", "colors", "--steps", "4"])
        .output()
        .expect("failed to run monlin debug colors");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert!(
        lines.len() >= 10,
        "unexpected debug colors output: {stdout}"
    );
    assert!(lines.iter().any(|line| line.starts_with("cpu ")));
    assert!(lines.iter().any(|line| line.starts_with("net ")));
}

#[test]
fn once_mode_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args([
            "--once",
            "--interval-ms",
            "0",
            "--width",
            "48",
            "--color",
            "never",
        ])
        .output()
        .expect("failed to run monlin --once");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cpu"));
    assert!(stdout.lines().count() >= 2);
}

#[test]
fn all_layout_renders_multiple_rows() {
    let output = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args([
            "--once",
            "--interval-ms",
            "0",
            "--width",
            "80",
            "--color",
            "never",
            "all",
        ])
        .output()
        .expect("failed to run monlin all");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().count() >= 2);
}

#[test]
fn i3bar_once_mode_emits_i3bar_protocol() {
    let output = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args([
            "--once",
            "--interval-ms",
            "0",
            "--width",
            "80",
            "--color",
            "never",
            "--output",
            "i3bar",
            "all",
        ])
        .output()
        .expect("failed to run monlin --output i3bar");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<_> = stdout.lines().collect();

    assert_eq!(lines.len(), 4, "unexpected i3bar once output: {stdout}");

    let header: Value = serde_json::from_str(lines[0]).expect("invalid i3bar header JSON");
    assert_eq!(header["version"], 1);
    assert_eq!(header["click_events"], false);
    assert_eq!(lines[1], "[");
    assert_eq!(lines[3], "]");

    let frame: Value = serde_json::from_str(lines[2]).expect("invalid i3bar frame JSON");
    let blocks = frame.as_array().expect("i3bar frame must be an array");
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0]["name"], "monlin-0");
    assert_eq!(blocks[1]["name"], "monlin-1");
    assert_eq!(blocks[0]["separator"], false);
    assert_eq!(blocks[1]["separator"], false);
    assert_eq!(blocks[0]["separator_block_width"], 0);
    assert_eq!(blocks[1]["separator_block_width"], 0);
    assert!(blocks[0]["full_text"]
        .as_str()
        .is_some_and(|text| !text.is_empty()));
    assert!(blocks[1]["full_text"]
        .as_str()
        .is_some_and(|text| !text.is_empty()));
}

#[test]
fn i3bar_once_mode_uses_one_block_per_row_without_ansi() {
    let output = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args([
            "--once",
            "--interval-ms",
            "0",
            "--width",
            "48",
            "--color",
            "always",
            "--output",
            "i3bar",
            "cpu, ram",
        ])
        .output()
        .expect("failed to run monlin --output i3bar 'cpu, ram'");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let frame: Value =
        serde_json::from_str(stdout.lines().nth(2).expect("missing i3bar frame line"))
            .expect("invalid i3bar frame JSON");
    let blocks = frame.as_array().expect("i3bar frame must be an array");

    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0]["name"], "monlin-0");
    assert_eq!(blocks[1]["name"], "monlin-1");

    for block in blocks {
        let text = block["full_text"]
            .as_str()
            .expect("i3bar block full_text must be a string");
        assert!(!text.is_empty());
        assert!(
            !text.contains('\u{1b}'),
            "unexpected ANSI escape in {text:?}"
        );
    }
}

#[test]
fn stdin_numeric_rows_enable_stream_mode_automatically() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args(["--once", "--width", "32", "--color", "never"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn monlin for stdin stream mode");

    child
        .stdin
        .as_mut()
        .expect("missing child stdin")
        .write_all(b"0 25 50\n")
        .expect("failed to write numeric stream input");

    let output = child
        .wait_with_output()
        .expect("failed waiting for monlin stream output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1, "unexpected stream output: {stdout}");
    assert!(lines[0].contains("0%"), "unexpected row: {stdout}");
    assert!(lines[0].contains("25%"), "unexpected row: {stdout}");
    assert!(lines[0].contains("50%"), "unexpected row: {stdout}");
}

#[test]
fn dash_forces_stdin_stream_mode() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args(["-", "--once", "--width", "32", "--color", "never"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn monlin -");

    child
        .stdin
        .as_mut()
        .expect("missing child stdin")
        .write_all(b"10 20\n")
        .expect("failed to write numeric stream input");

    let output = child
        .wait_with_output()
        .expect("failed waiting for monlin - output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1, "unexpected stream output: {stdout}");
    assert!(lines[0].contains("10%"), "unexpected row: {stdout}");
    assert!(lines[0].contains("20%"), "unexpected row: {stdout}");
}

#[test]
fn stream_mode_uses_explicit_labels() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args([
            "-", "--once", "--width", "32", "--color", "never", "--labels", "wifi,vpn",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn monlin - --labels");

    child
        .stdin
        .as_mut()
        .expect("missing child stdin")
        .write_all(b"10 20\n")
        .expect("failed to write numeric stream input");

    let output = child
        .wait_with_output()
        .expect("failed waiting for monlin labeled stream output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1, "unexpected stream output: {stdout}");
    assert!(lines[0].contains("wifi"), "unexpected row: {stdout}");
    assert!(lines[0].contains("vpn"), "unexpected row: {stdout}");
    assert!(lines[0].contains("10%"), "unexpected row: {stdout}");
    assert!(lines[0].contains("20%"), "unexpected row: {stdout}");
}

#[test]
fn stream_mode_lines_layout_preserves_old_per_series_rows() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args([
            "-",
            "--once",
            "--width",
            "32",
            "--color",
            "never",
            "--stream-layout",
            "lines",
            "--labels",
            "wifi,vpn",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn monlin - --stream-layout lines");

    child
        .stdin
        .as_mut()
        .expect("missing child stdin")
        .write_all(b"10 20\n")
        .expect("failed to write numeric stream input");

    let output = child
        .wait_with_output()
        .expect("failed waiting for monlin line-layout stream output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2, "unexpected stream output: {stdout}");
    assert!(
        lines[0].starts_with("wifi  10%"),
        "unexpected first row: {stdout}"
    );
    assert!(
        lines[1].starts_with(" vpn  20%"),
        "unexpected second row: {stdout}"
    );
}

#[test]
fn stream_mode_rejects_mismatched_labels() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .args([
            "-",
            "--once",
            "--width",
            "32",
            "--color",
            "never",
            "--labels",
            "wifi,vpn,ts",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn monlin - with mismatched labels");

    child
        .stdin
        .as_mut()
        .expect("missing child stdin")
        .write_all(b"10 20\n")
        .expect("failed to write numeric stream input");

    let output = child
        .wait_with_output()
        .expect("failed waiting for monlin mismatched labels output");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--labels expected 2 entries"),
        "unexpected stderr: {stderr}"
    );
}

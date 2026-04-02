use std::process::Command;

use serde_json::Value;

#[test]
fn help_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_monlin"))
        .arg("--help")
        .output()
        .expect("failed to run monlin --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--layout"));
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
    assert!(stdout.contains("sys"));
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
            "--layout",
            "all",
        ])
        .output()
        .expect("failed to run monlin --layout all");

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
            "--layout",
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
            "--layout",
            "cpu, ram",
        ])
        .output()
        .expect("failed to run monlin --output i3bar --layout 'cpu, ram'");

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

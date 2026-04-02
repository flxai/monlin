use std::process::Command;

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
        .args(["--once", "--interval-ms", "0", "--width", "48", "--color", "never"])
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
            "all/2",
        ])
        .output()
        .expect("failed to run monlin --output i3bar");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"version\":1"));
    assert!(stdout.contains("\"click_events\":false"));
    assert!(stdout.contains("\"name\":\"monlin-0\""));
    assert!(stdout.contains("\"full_text\":\"sys"));
    assert!(stdout.contains('['));
    assert!(stdout.trim_end().ends_with(']'));
}

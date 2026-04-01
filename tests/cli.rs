use std::process::Command;

#[test]
fn help_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_nxu-cpu"))
        .arg("--help")
        .output()
        .expect("failed to run nxu-cpu --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--layout"));
}

#[test]
fn once_mode_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_nxu-cpu"))
        .args(["--once", "--interval-ms", "0", "--width", "48", "--color", "never"])
        .output()
        .expect("failed to run nxu-cpu --once");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cpu"));
}

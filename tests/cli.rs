use std::process::Command;

#[test]
fn help_exposes_supported_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_tyria-extractor-rs"))
        .arg("--help")
        .output()
        .expect("run tyria-extractor-rs --help");

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("help must be UTF-8");
    for command in ["extract", "dump-entries", "extract-entry"] {
        assert!(stdout.contains(command), "missing {command} in:\n{stdout}");
    }
}

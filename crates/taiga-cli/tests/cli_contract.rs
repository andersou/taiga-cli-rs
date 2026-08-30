use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn help_exposes_requested_command_groups() {
    let mut command = Command::cargo_bin("taiga").unwrap();
    command
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("project"))
        .stdout(contains("userstory"))
        .stdout(contains("notification"))
        .stdout(contains("timeline"));
}

#[test]
fn delete_requires_explicit_confirmation_before_network() {
    let mut command = Command::cargo_bin("taiga").unwrap();
    command
        .args(["project", "delete", "1"])
        .assert()
        .code(2)
        .stderr(contains("delete requires --yes"));
}

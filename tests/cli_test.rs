use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_lists_the_m1_commands() {
    Command::cargo_bin("platform")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("serve"))
        .stdout(predicate::str::contains("deploy"))
        .stdout(predicate::str::contains("project"));
}

#[test]
fn project_create_requires_name_source_and_base_image() {
    Command::cargo_bin("platform")
        .unwrap()
        .args(["project", "create"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

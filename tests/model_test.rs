use deploy_platform::{
    config::parse_server_command,
    model::{DeploymentStatus, Framework, PlatformState},
};

#[test]
fn empty_platform_state_serializes_with_version_one() {
    let json = serde_json::to_string(&PlatformState::empty()).unwrap();
    assert!(json.contains("\"version\":1"));
    assert!(json.contains("\"projects\":[]"));
    assert!(json.contains("\"deployments\":[]"));
}

#[test]
fn deployment_status_uses_stable_lowercase_json_names() {
    let json = serde_json::to_string(&DeploymentStatus::Running).unwrap();
    assert_eq!(json, "\"running\"");
}

#[test]
fn server_command_splits_ascii_whitespace_without_shell_expansion() {
    let command = parse_server_command("/bin/busybox httpd -p {PORT} -h /srv/app").unwrap();
    assert_eq!(
        command,
        ["/bin/busybox", "httpd", "-p", "{PORT}", "-h", "/srv/app"]
    );
}

#[test]
fn empty_server_command_is_rejected() {
    let error = parse_server_command("   ").unwrap_err();
    assert!(error.to_string().contains("server command cannot be empty"));
}

#[test]
fn framework_has_only_the_m1_static_variant() {
    assert_eq!(
        serde_json::to_string(&Framework::Static).unwrap(),
        "\"static\""
    );
}

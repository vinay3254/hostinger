use deploy_platform::runtime::{discover_new_container, render_server_command};
use minidock::state::{ContainerState, ContainerStatus};
use std::path::PathBuf;
use time::OffsetDateTime;
use uuid::Uuid;

fn state(hostname: &str, command: &[&str]) -> ContainerState {
    ContainerState {
        version: 1,
        id: Uuid::new_v4(),
        pid: 123,
        cgroup_path: PathBuf::new(),
        rootfs: PathBuf::new(),
        command: command.iter().map(|item| (*item).into()).collect(),
        hostname: hostname.into(),
        detached: true,
        started_at: OffsetDateTime::now_utc(),
        status: ContainerStatus::Running,
    }
}

#[test]
fn replaces_only_a_complete_port_token() {
    let template = ["/bin/busybox", "-p", "{PORT}", "-h", "/srv/app"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let result = render_server_command(&template, 43123).unwrap();
    assert_eq!(
        result,
        vec!["/bin/busybox", "-p", "43123", "-h", "/srv/app"]
    );
}

#[test]
fn rejects_missing_or_ambiguous_port_tokens() {
    let missing = vec!["server".to_string()];
    let ambiguous = vec![
        "server".to_string(),
        "{PORT}".to_string(),
        "{PORT}".to_string(),
    ];
    let embedded = vec!["server=:{PORT}".to_string()];
    assert!(render_server_command(&missing, 8080)
        .unwrap_err()
        .to_string()
        .contains("{PORT}"));
    assert!(render_server_command(&ambiguous, 8080)
        .unwrap_err()
        .to_string()
        .contains("exactly one"));
    assert!(render_server_command(&embedded, 8080)
        .unwrap_err()
        .to_string()
        .contains("complete token"));
}

#[test]
fn discovers_only_the_new_matching_container_state() {
    let old = state("platform-old", &["server", "1"]);
    let new = state("platform-new", &["server", "43123"]);
    let command = vec!["server".to_string(), "43123".to_string()];
    let before = [old.clone()];
    let after = [old, new.clone()];
    let found = discover_new_container(&before, &after, "platform-new", &command).unwrap();
    assert_eq!(found.id, new.id);
}

#[test]
fn ambiguous_container_discovery_fails() {
    let first = state("same-hostname", &["server", "43123"]);
    let second = state("same-hostname", &["server", "43123"]);
    let command = vec!["server".to_string(), "43123".to_string()];
    let error =
        discover_new_container(&[], &[first, second], "same-hostname", &command).unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
}

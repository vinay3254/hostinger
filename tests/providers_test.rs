use deploy_platform::providers::{
    parse_github_webhook, parse_gitlab_webhook, Provider, PullRequestAction, PullRequestRef,
    RepositoryRef, SourceEventKind,
};
use url::Url;

#[test]
fn github_push_event_normalizes_correctly() {
    let payload = serde_json::json!({
        "ref": "refs/heads/main",
        "after": "6dcb09b5b57875f334f61aebed695e2e4193db5e",
        "repository": {
            "id": 123456,
            "clone_url": "https://github.com/example/repo.git",
            "default_branch": "main"
        }
    });

    let event = parse_github_webhook("push", "delivery-123", payload.to_string().as_bytes())
        .expect("failed to parse github push");

    assert_eq!(event.delivery_id, "delivery-123");
    assert_eq!(event.kind, SourceEventKind::Push);
    assert_eq!(event.commit_sha, "6dcb09b5b57875f334f61aebed695e2e4193db5e");
    assert_eq!(event.branch.as_deref(), Some("main"));
    assert_eq!(event.pull_request, None);
    assert_eq!(
        event.repository,
        RepositoryRef {
            provider: Provider::GitHub,
            external_id: "123456".into(),
            clone_url: Url::parse("https://github.com/example/repo.git").unwrap(),
            default_branch: "main".into(),
        }
    );
}

#[test]
fn github_pull_request_events_normalize_correctly() {
    for (action, expected_kind, expected_action) in [
        (
            "opened",
            SourceEventKind::PullRequestOpened,
            PullRequestAction::Opened,
        ),
        (
            "synchronize",
            SourceEventKind::PullRequestUpdated,
            PullRequestAction::Synchronize,
        ),
        (
            "closed",
            SourceEventKind::PullRequestClosed,
            PullRequestAction::Closed,
        ),
    ] {
        let payload = serde_json::json!({
            "action": action,
            "number": 42,
            "pull_request": {
                "head": {
                    "sha": "1111222233334444555566667777888899990000",
                    "ref": "feature-branch"
                },
                "base": {
                    "ref": "main"
                }
            },
            "repository": {
                "id": 987654,
                "clone_url": "https://github.com/example/repo.git",
                "default_branch": "main"
            }
        });

        let event = parse_github_webhook(
            "pull_request",
            &format!("pr-delivery-{action}"),
            payload.to_string().as_bytes(),
        )
        .expect("failed to parse github pr");

        assert_eq!(event.delivery_id, format!("pr-delivery-{action}"));
        assert_eq!(event.kind, expected_kind);
        assert_eq!(event.commit_sha, "1111222233334444555566667777888899990000");
        assert_eq!(event.branch.as_deref(), Some("feature-branch"));
        assert_eq!(
            event.pull_request,
            Some(PullRequestRef {
                number: 42,
                head_sha: "1111222233334444555566667777888899990000".into(),
                base_branch: "main".into(),
                action: expected_action,
            })
        );
    }
}

#[test]
fn gitlab_push_and_merge_request_normalize_correctly() {
    // GitLab Push
    let push_payload = serde_json::json!({
        "ref": "refs/heads/release",
        "checkout_sha": "aabbccddeeff00112233445566778899aabbccdd",
        "project": {
            "id": 5555,
            "git_http_url": "https://gitlab.com/example/project.git",
            "default_branch": "main"
        }
    });

    let push_event = parse_gitlab_webhook(
        "Push Hook",
        "gl-delivery-1",
        push_payload.to_string().as_bytes(),
    )
    .expect("failed to parse gitlab push");

    assert_eq!(push_event.kind, SourceEventKind::Push);
    assert_eq!(
        push_event.commit_sha,
        "aabbccddeeff00112233445566778899aabbccdd"
    );
    assert_eq!(push_event.branch.as_deref(), Some("release"));
    assert_eq!(push_event.repository.provider, Provider::GitLab);

    // GitLab MR
    let mr_payload = serde_json::json!({
        "object_attributes": {
            "action": "open",
            "iid": 10,
            "last_commit": {
                "id": "7777888899990000111122223333444455556666"
            },
            "source_branch": "patch-1",
            "target_branch": "main"
        },
        "project": {
            "id": 5555,
            "git_http_url": "https://gitlab.com/example/project.git",
            "default_branch": "main"
        }
    });

    let mr_event = parse_gitlab_webhook(
        "Merge Request Hook",
        "gl-delivery-2",
        mr_payload.to_string().as_bytes(),
    )
    .expect("failed to parse gitlab mr");

    assert_eq!(mr_event.kind, SourceEventKind::PullRequestOpened);
    assert_eq!(
        mr_event.commit_sha,
        "7777888899990000111122223333444455556666"
    );
    assert_eq!(mr_event.branch.as_deref(), Some("patch-1"));
    assert_eq!(
        mr_event.pull_request,
        Some(PullRequestRef {
            number: 10,
            head_sha: "7777888899990000111122223333444455556666".into(),
            base_branch: "main".into(),
            action: PullRequestAction::Opened,
        })
    );
}

#[test]
fn rejects_missing_repository_or_commit_identity() {
    let no_repo = serde_json::json!({
        "ref": "refs/heads/main",
        "after": "6dcb09b5b57875f334f61aebed695e2e4193db5e"
    });
    assert!(parse_github_webhook("push", "d1", no_repo.to_string().as_bytes()).is_err());

    let no_commit = serde_json::json!({
        "ref": "refs/heads/main",
        "repository": {
            "id": 123456,
            "clone_url": "https://github.com/example/repo.git",
            "default_branch": "main"
        }
    });
    assert!(parse_github_webhook("push", "d2", no_commit.to_string().as_bytes()).is_err());
}

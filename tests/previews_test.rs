use deploy_platform::{
    db::Database,
    previews::{PreviewRepository, PreviewStatus, UpsertPreviewInput},
};
use time::OffsetDateTime;
use uuid::Uuid;

async fn setup_test_db() -> Database {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/deploy_platform".into());
    let db = Database::connect(&url)
        .await
        .expect("failed to connect to test database");
    db.migrate().await.expect("failed to run migrations");
    db
}

async fn create_test_user_and_project(db: &Database) -> (Uuid, Uuid) {
    let user_id = Uuid::new_v4();
    let email = format!("user-{}@example.com", user_id);
    sqlx::query("INSERT INTO users (id, email, password_hash, name, created_at) VALUES ($1, $2, 'hash', 'Test', NOW())")
        .bind(user_id)
        .bind(email)
        .execute(db.pool())
        .await
        .unwrap();

    let project_id = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, user_id, name, source_dir, base_image, server_command, created_at) VALUES ($1, $2, $3, '/tmp', 'alpine', ARRAY['echo'], NOW())")
        .bind(project_id)
        .bind(user_id)
        .bind(format!("proj-{}", project_id))
        .execute(db.pool())
        .await
        .unwrap();

    (user_id, project_id)
}

#[tokio::test]
async fn preview_identity_uniqueness_and_lifecycle() {
    let db = setup_test_db().await;
    let (_user_id, project_id) = create_test_user_and_project(&db).await;

    let mut repo = PreviewRepository::new(db.pool().into());

    let input1 = UpsertPreviewInput {
        project_id,
        provider: "github".to_string(),
        pr_number: 101,
        head_sha: "abc111".to_string(),
        base_branch: "main".to_string(),
        head_branch: "feature-auth".to_string(),
        deployment_id: None,
        hostname: "app-pr-101.preview.local".to_string(),
        status: PreviewStatus::Building,
    };

    let preview1 = repo.upsert(&input1).await.expect("initial preview created");
    assert_eq!(preview1.project_id, project_id);
    assert_eq!(preview1.pr_number, 101);
    assert_eq!(preview1.head_sha, "abc111");
    assert_eq!(preview1.status, PreviewStatus::Building);

    // Upserting for the same (project_id, provider, pr_number) updates the existing record
    let input2 = UpsertPreviewInput {
        project_id,
        provider: "github".to_string(),
        pr_number: 101,
        head_sha: "def222".to_string(),
        base_branch: "main".to_string(),
        head_branch: "feature-auth".to_string(),
        deployment_id: None,
        hostname: "app-pr-101.preview.local".to_string(),
        status: PreviewStatus::Ready,
    };

    let preview2 = repo.upsert(&input2).await.expect("preview updated");
    assert_eq!(
        preview2.id, preview1.id,
        "must update the same preview record"
    );
    assert_eq!(preview2.head_sha, "def222");
    assert_eq!(preview2.status, PreviewStatus::Ready);

    // Different PR number creates a distinct preview record
    let input3 = UpsertPreviewInput {
        project_id,
        provider: "github".to_string(),
        pr_number: 102,
        head_sha: "ghi333".to_string(),
        base_branch: "main".to_string(),
        head_branch: "feature-ui".to_string(),
        deployment_id: None,
        hostname: "app-pr-102.preview.local".to_string(),
        status: PreviewStatus::Building,
    };

    let preview3 = repo
        .upsert(&input3)
        .await
        .expect("distinct preview created");
    assert_ne!(preview3.id, preview1.id);

    // Mark closed and cleanup attempt
    repo.mark_closed(preview1.id, OffsetDateTime::now_utc())
        .await
        .expect("marked closed");

    let fetched = repo.find_by_id(preview1.id).await.unwrap().expect("found");
    assert_eq!(fetched.status, PreviewStatus::Closed);
    assert!(fetched.closed_at.is_some());

    repo.increment_cleanup_attempt(preview1.id)
        .await
        .expect("cleanup attempt incremented");

    let after_cleanup = repo.find_by_id(preview1.id).await.unwrap().expect("found");
    assert_eq!(after_cleanup.cleanup_attempt, 1);
}

use deploy_platform::{
    preview_events::{PreviewOutcome, PreviewService},
    providers::{Provider, PullRequestAction, PullRequestRef, SourceEventKind},
    source_events::SourceEventRecord,
};

#[tokio::test]
async fn pull_request_event_lifecycle_and_idempotency() {
    let db = setup_test_db().await;
    let (_user_id, project_id) = create_test_user_and_project(&db).await;

    let service = PreviewService::new(db.clone(), None);

    let base_time = OffsetDateTime::now_utc();

    // 1. PR Opened -> Created
    let pr_ref1 = PullRequestRef {
        number: 42,
        head_sha: "commit-sha-1".to_string(),
        base_branch: "main".to_string(),
        action: PullRequestAction::Opened,
    };
    let event1 = SourceEventRecord {
        id: Uuid::new_v4(),
        project_id,
        provider: Provider::GitHub,
        delivery_id: "deliv-1".to_string(),
        kind: SourceEventKind::PullRequestOpened,
        commit_sha: "commit-sha-1".to_string(),
        branch: Some("feature/login".to_string()),
        pull_request: Some(pr_ref1),
        idempotency_key: "github:deliv-1:commit-sha-1".to_string(),
        created_at: base_time,
    };

    let outcome1 = service
        .apply_event(&event1)
        .await
        .expect("applied event 1")
        .expect("expected outcome");
    let (preview_id, dep1) = match outcome1 {
        PreviewOutcome::Created {
            preview_id,
            deployment_id,
        } => (preview_id, deployment_id),
        other => panic!("expected Created outcome, got {:?}", other),
    };

    let mut repo = PreviewRepository::new(db.pool().into());
    let preview = repo.find_by_id(preview_id).await.unwrap().unwrap();
    assert_eq!(preview.status, PreviewStatus::Building);
    assert_eq!(preview.deployment_id, Some(dep1));
    assert_eq!(preview.head_sha, "commit-sha-1");
    assert!(preview.hostname.contains("pr-42"));

    // 2. PR Synchronized (new commit) -> Updated
    let pr_ref2 = PullRequestRef {
        number: 42,
        head_sha: "commit-sha-2".to_string(),
        base_branch: "main".to_string(),
        action: PullRequestAction::Synchronize,
    };
    let event2 = SourceEventRecord {
        id: Uuid::new_v4(),
        project_id,
        provider: Provider::GitHub,
        delivery_id: "deliv-2".to_string(),
        kind: SourceEventKind::PullRequestUpdated,
        commit_sha: "commit-sha-2".to_string(),
        branch: Some("feature/login".to_string()),
        pull_request: Some(pr_ref2),
        idempotency_key: "github:deliv-2:commit-sha-2".to_string(),
        created_at: base_time + time::Duration::seconds(10),
    };

    let outcome2 = service
        .apply_event(&event2)
        .await
        .expect("applied event 2")
        .expect("expected outcome");
    let dep2 = match outcome2 {
        PreviewOutcome::Updated {
            preview_id: p_id,
            deployment_id,
        } => {
            assert_eq!(p_id, preview_id);
            deployment_id
        }
        other => panic!("expected Updated outcome, got {:?}", other),
    };
    assert_ne!(dep1, dep2);

    let preview = repo.find_by_id(preview_id).await.unwrap().unwrap();
    assert_eq!(preview.head_sha, "commit-sha-2");
    assert_eq!(preview.deployment_id, Some(dep2));

    // 3. Duplicate delivery of event 2 -> Unchanged
    let outcome_dup = service
        .apply_event(&event2)
        .await
        .expect("applied duplicate")
        .expect("expected outcome");
    match outcome_dup {
        PreviewOutcome::Unchanged { preview_id: p_id } => assert_eq!(p_id, preview_id),
        other => panic!("expected Unchanged outcome, got {:?}", other),
    }

    // 4. Older event arriving late -> IgnoredOlder
    let pr_ref_old = PullRequestRef {
        number: 42,
        head_sha: "commit-sha-0".to_string(),
        base_branch: "main".to_string(),
        action: PullRequestAction::Synchronize,
    };
    let event_old = SourceEventRecord {
        id: Uuid::new_v4(),
        project_id,
        provider: Provider::GitHub,
        delivery_id: "deliv-old".to_string(),
        kind: SourceEventKind::PullRequestUpdated,
        commit_sha: "commit-sha-0".to_string(),
        branch: Some("feature/login".to_string()),
        pull_request: Some(pr_ref_old),
        idempotency_key: "github:deliv-old:commit-sha-0".to_string(),
        created_at: base_time - time::Duration::seconds(5),
    };
    let outcome_old = service
        .apply_event(&event_old)
        .await
        .expect("applied older event")
        .expect("expected outcome");
    match outcome_old {
        PreviewOutcome::IgnoredOlder { preview_id: p_id } => assert_eq!(p_id, preview_id),
        other => panic!("expected IgnoredOlder outcome, got {:?}", other),
    }
    let preview = repo.find_by_id(preview_id).await.unwrap().unwrap();
    assert_eq!(preview.head_sha, "commit-sha-2");

    // 5. PR Closed -> Closed
    let pr_ref_closed = PullRequestRef {
        number: 42,
        head_sha: "commit-sha-2".to_string(),
        base_branch: "main".to_string(),
        action: PullRequestAction::Closed,
    };
    let event_closed = SourceEventRecord {
        id: Uuid::new_v4(),
        project_id,
        provider: Provider::GitHub,
        delivery_id: "deliv-closed".to_string(),
        kind: SourceEventKind::PullRequestClosed,
        commit_sha: "commit-sha-2".to_string(),
        branch: Some("feature/login".to_string()),
        pull_request: Some(pr_ref_closed),
        idempotency_key: "github:deliv-closed:commit-sha-2".to_string(),
        created_at: base_time + time::Duration::seconds(20),
    };
    let outcome_closed = service
        .apply_event(&event_closed)
        .await
        .expect("applied closed event")
        .expect("expected outcome");
    match outcome_closed {
        PreviewOutcome::Closed { preview_id: p_id } => assert_eq!(p_id, preview_id),
        other => panic!("expected Closed outcome, got {:?}", other),
    }
    let preview = repo.find_by_id(preview_id).await.unwrap().unwrap();
    assert_eq!(preview.status, PreviewStatus::Closed);
    assert!(preview.closed_at.is_some());

    // 6. PR Reopened -> Updated/Reopened
    let pr_ref_reopened = PullRequestRef {
        number: 42,
        head_sha: "commit-sha-3".to_string(),
        base_branch: "main".to_string(),
        action: PullRequestAction::Reopened,
    };
    let event_reopened = SourceEventRecord {
        id: Uuid::new_v4(),
        project_id,
        provider: Provider::GitHub,
        delivery_id: "deliv-reopened".to_string(),
        kind: SourceEventKind::PullRequestOpened,
        commit_sha: "commit-sha-3".to_string(),
        branch: Some("feature/login".to_string()),
        pull_request: Some(pr_ref_reopened),
        idempotency_key: "github:deliv-reopened:commit-sha-3".to_string(),
        created_at: base_time + time::Duration::seconds(30),
    };
    let outcome_reopened = service
        .apply_event(&event_reopened)
        .await
        .expect("applied reopened event")
        .expect("expected outcome");
    match outcome_reopened {
        PreviewOutcome::Updated {
            preview_id: p_id,
            deployment_id,
        } => {
            assert_eq!(p_id, preview_id);
            assert_ne!(deployment_id, dep2);
        }
        other => panic!("expected Updated outcome on reopen, got {:?}", other),
    }
    let preview = repo.find_by_id(preview_id).await.unwrap().unwrap();
    assert_eq!(preview.status, PreviewStatus::Building);
    assert_eq!(preview.head_sha, "commit-sha-3");
}

#[tokio::test]
async fn preview_runtime_lifecycle_and_promotion() {
    let db = setup_test_db().await;
    let (_user_id, project_id) = create_test_user_and_project(&db).await;

    let service = PreviewService::new(db.clone(), None);
    let mut repo = PreviewRepository::new(db.pool().into());

    // 1. Initial PR Event creates preview
    let pr_ref = PullRequestRef {
        number: 55,
        head_sha: "rc-sha-1".to_string(),
        base_branch: "main".to_string(),
        action: PullRequestAction::Opened,
    };
    let event1 = SourceEventRecord {
        id: Uuid::new_v4(),
        project_id,
        provider: Provider::GitHub,
        delivery_id: "deliv-55-1".to_string(),
        kind: SourceEventKind::PullRequestOpened,
        commit_sha: "rc-sha-1".to_string(),
        branch: Some("feature/release".to_string()),
        pull_request: Some(pr_ref),
        idempotency_key: "github:deliv-55-1:rc-sha-1".to_string(),
        created_at: OffsetDateTime::now_utc(),
    };

    let outcome = service.apply_event(&event1).await.unwrap().unwrap();
    let (preview_id, dep1) = match outcome {
        PreviewOutcome::Created {
            preview_id,
            deployment_id,
        } => (preview_id, deployment_id),
        _ => panic!("expected Created"),
    };

    // 2. Preview build success allocates route and updates preview to Ready
    let preview_url = "http://app-pr-55.preview.local:4000";
    service
        .record_build_success(preview_id, dep1, 4000, preview_url, "/artifacts/dep1.tar")
        .await
        .expect("recorded build success");

    let p = repo.find_by_id(preview_id).await.unwrap().unwrap();
    assert_eq!(p.status, PreviewStatus::Ready);
    assert_eq!(p.deployment_id, Some(dep1));

    let d1: (String, Option<i32>, Option<String>) =
        sqlx::query_as("SELECT status, port, url FROM deployments WHERE id = $1")
            .bind(dep1)
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(d1.0, "running");
    assert_eq!(d1.1, Some(4000));
    assert_eq!(d1.2.as_deref(), Some(preview_url));

    // 3. New commit build fails -> Failed build preserves prior healthy preview!
    let dep2 = Uuid::new_v4();
    sqlx::query("INSERT INTO deployments (id, project_id, framework, status, created_at, commit_sha) VALUES ($1, $2, 'static', 'building', NOW(), 'rc-sha-2')")
        .bind(dep2)
        .bind(project_id)
        .execute(db.pool())
        .await
        .unwrap();

    service
        .record_build_failure(preview_id, dep2, "compilation failed on step 3")
        .await
        .expect("recorded build failure");

    // Failed deployment is marked failed
    let d2_status: String = sqlx::query_scalar("SELECT status FROM deployments WHERE id = $1")
        .bind(dep2)
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(d2_status, "failed");

    // Prior preview MUST remain Ready with prior deployment dep1 preserved
    let p_preserved = repo.find_by_id(preview_id).await.unwrap().unwrap();
    assert_eq!(p_preserved.status, PreviewStatus::Ready);
    assert_eq!(p_preserved.deployment_id, Some(dep1));

    // 4. Promote to production creates a new production deployment from exact preview commit
    let prod_dep = service
        .promote_to_production(preview_id)
        .await
        .expect("promoted preview to production");

    assert_ne!(
        prod_dep.id, dep1,
        "promoted deployment must be a new deployment record"
    );
    assert_eq!(prod_dep.project_id, project_id);
    assert_eq!(prod_dep.commit_sha.as_deref(), Some("rc-sha-1"));

    // Preview remains unchanged and ready
    let p_after_promote = repo.find_by_id(preview_id).await.unwrap().unwrap();
    assert_eq!(p_after_promote.status, PreviewStatus::Ready);

    // 5. Idempotent Teardown
    service
        .teardown_preview(preview_id)
        .await
        .expect("teardown succeeds");
    let p_closed = repo.find_by_id(preview_id).await.unwrap().unwrap();
    assert_eq!(p_closed.status, PreviewStatus::Closed);
    assert!(p_closed.closed_at.is_some());

    let d1_after_stop: String = sqlx::query_scalar("SELECT status FROM deployments WHERE id = $1")
        .bind(dep1)
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(d1_after_stop, "stopped");

    // Second teardown call is idempotent
    service
        .teardown_preview(preview_id)
        .await
        .expect("idempotent teardown succeeds");
}

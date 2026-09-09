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

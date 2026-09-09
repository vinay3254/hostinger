use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use deploy_platform::build_executor::{LogSink, LogStream};
use deploy_platform::db::Database;
use deploy_platform::logs::{DurableLogSink, LogQuery, Redactor};

async fn setup_test_db() -> Database {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/deploy_platform".into());
    let db = Database::connect(&url)
        .await
        .expect("failed to connect to test database");
    db.migrate().await.expect("failed to run migrations");
    db
}

async fn create_project_and_deployment(db: &Database) -> (Uuid, Uuid) {
    let user_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, name, created_at)
         VALUES ($1, $2, 'hash', 'Test User', NOW())",
    )
    .bind(user_id)
    .bind(format!("log-test-{}@example.com", Uuid::new_v4().simple()))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO projects (id, user_id, name, source_dir, base_image, server_command, created_at)
         VALUES ($1, $2, $3, '/tmp', '/tmp', ARRAY['echo'], NOW())",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(format!("log-proj-{}", Uuid::new_v4().simple()))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, created_at)
         VALUES ($1, $2, 'static', 'running', NOW())",
    )
    .bind(deployment_id)
    .bind(project_id)
    .execute(db.pool())
    .await
    .unwrap();

    (project_id, deployment_id)
}

#[test]
fn test_redaction_secrets_and_common_patterns() {
    let redactor = Redactor::new(vec![
        "super_secret_password_123".to_string(),
        "another-api-token".to_string(),
    ]);

    // 1. Configured secret fingerprints
    let input = "Connecting with super_secret_password_123 and another-api-token";
    let output = redactor.redact(input);
    assert_eq!(output, "Connecting with [REDACTED] and [REDACTED]");

    // 2. Bearer tokens
    let input_bearer = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.xyz";
    let out_bearer = redactor.redact(input_bearer);
    assert_eq!(out_bearer, "Authorization: Bearer [REDACTED]");

    // 3. AWS keys
    let input_aws = "Using AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE for s3";
    let out_aws = redactor.redact(input_aws);
    assert_eq!(out_aws, "Using AWS_ACCESS_KEY_ID=[REDACTED] for s3");

    // 4. GitHub tokens
    let input_gh = "git clone https://ghp_123456789012345678901234567890123456@github.com/repo";
    let out_gh = redactor.redact(input_gh);
    assert_eq!(out_gh, "git clone https://[REDACTED]@github.com/repo");

    // 5. Password and token assignments
    let input_assign = "Setting password=mysecretpass and token:myapitoken in config";
    let out_assign = redactor.redact(input_assign);
    assert_eq!(
        out_assign,
        "Setting password=[REDACTED] and token:[REDACTED] in config"
    );

    // 6. Private keys
    let priv_key = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA0fakekeydata...\n-----END RSA PRIVATE KEY-----";
    let out_pk = redactor.redact(priv_key);
    assert_eq!(out_pk, "[REDACTED PRIVATE KEY]");
}

#[tokio::test]
async fn test_log_sink_monotonic_sequence_and_ordering() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;

    let redactor = Redactor::default();
    let mut sink = DurableLogSink::new(
        deployment_id,
        project_id,
        db.pool().clone(),
        redactor,
        None,
        100,
    );

    // Write 5 lines across stdout and stderr
    sink.write_line(LogStream::Stdout, "Step 1: Cloning repository")
        .unwrap();
    sink.write_line(LogStream::Stdout, "Step 2: Installing dependencies")
        .unwrap();
    sink.write_line(LogStream::Stderr, "Warning: deprecated package")
        .unwrap();
    sink.write_line(LogStream::Stdout, "Step 3: Building bundle")
        .unwrap();
    sink.write_line(LogStream::Stdout, "Build complete!")
        .unwrap();

    assert_eq!(sink.current_sequence(), 5);

    // Wait for all async DB inserts to complete
    sink.wait_all().await;

    let mut repo = db.logs();
    let response = repo
        .query_logs(deployment_id, &LogQuery::default())
        .await
        .unwrap();

    assert_eq!(response.lines.len(), 5);
    for (i, line) in response.lines.iter().enumerate() {
        assert_eq!(line.sequence, (i + 1) as u64);
    }
    assert_eq!(response.lines[0].message, "Step 1: Cloning repository");
    assert_eq!(response.lines[2].stream, "stderr");
    assert_eq!(response.lines[4].message, "Build complete!");
}

#[tokio::test]
async fn test_log_query_pagination_and_reconnect() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;

    let now = OffsetDateTime::now_utc();
    let mut repo = db.logs();

    // Insert 25 log entries
    for seq in 1..=25 {
        repo.append(
            deployment_id,
            project_id,
            seq,
            "stdout",
            &format!("Log line {seq}"),
            now + Duration::seconds(seq as i64),
        )
        .await
        .unwrap();
    }

    // Page 1: limit 10, after_sequence None -> lines 1..=10, has_more true, next_sequence 10
    let page1 = repo
        .query_logs(
            deployment_id,
            &LogQuery {
                after_sequence: None,
                limit: Some(10),
                stream: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(page1.lines.len(), 10);
    assert_eq!(page1.lines.first().unwrap().sequence, 1);
    assert_eq!(page1.lines.last().unwrap().sequence, 10);
    assert!(page1.has_more);
    assert_eq!(page1.next_sequence, Some(10));

    // Page 2: reconnect from after_sequence 10 -> lines 11..=20
    let page2 = repo
        .query_logs(
            deployment_id,
            &LogQuery {
                after_sequence: Some(10),
                limit: Some(10),
                stream: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(page2.lines.len(), 10);
    assert_eq!(page2.lines.first().unwrap().sequence, 11);
    assert_eq!(page2.lines.last().unwrap().sequence, 20);
    assert!(page2.has_more);
    assert_eq!(page2.next_sequence, Some(20));

    // Page 3: after_sequence 20 -> lines 21..=25, has_more false
    let page3 = repo
        .query_logs(
            deployment_id,
            &LogQuery {
                after_sequence: Some(20),
                limit: Some(10),
                stream: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(page3.lines.len(), 5);
    assert_eq!(page3.lines.first().unwrap().sequence, 21);
    assert_eq!(page3.lines.last().unwrap().sequence, 25);
    assert!(!page3.has_more);
    assert_eq!(page3.next_sequence, Some(25));
}

#[tokio::test]
async fn test_log_retention_purge() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;

    let now = OffsetDateTime::now_utc();
    let old_time = now - Duration::days(45);
    let recent_time = now - Duration::days(2);

    let mut repo = db.logs();

    // Insert 5 old logs
    for seq in 1..=5 {
        repo.append(
            deployment_id,
            project_id,
            seq,
            "stdout",
            &format!("Old log {seq}"),
            old_time,
        )
        .await
        .unwrap();
    }

    // Insert 5 fresh logs
    for seq in 6..=10 {
        repo.append(
            deployment_id,
            project_id,
            seq,
            "stdout",
            &format!("Fresh log {seq}"),
            recent_time,
        )
        .await
        .unwrap();
    }

    // Retention cutoff: 30 days ago
    let cutoff = now - Duration::days(30);
    let purged = repo.purge_logs_older_than(cutoff).await.unwrap();
    assert_eq!(purged, 5);

    // Only fresh logs remain
    let remaining = repo
        .query_logs(deployment_id, &LogQuery::default())
        .await
        .unwrap();
    assert_eq!(remaining.lines.len(), 5);
    assert_eq!(remaining.lines[0].sequence, 6);
}

#[tokio::test]
async fn test_log_segment_rotation_and_download() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;

    let temp = tempdir().unwrap();
    let redactor = Redactor::default();

    // Set threshold to 5 lines
    let mut sink = DurableLogSink::new(
        deployment_id,
        project_id,
        db.pool().clone(),
        redactor,
        Some(temp.path().to_path_buf()),
        5,
    );

    // Write 12 lines
    for i in 1..=12 {
        sink.write_line(LogStream::Stdout, &format!("Build step {i}"))
            .unwrap();
    }

    // Explicit flush for the remaining 2 lines
    sink.flush().unwrap();

    // Wait for all async DB recording to complete
    sink.wait_all().await;

    // Check files created on disk
    let dep_dir = temp.path().join(deployment_id.to_string());
    assert!(dep_dir.join("00000001-00000005.log").is_file());
    assert!(dep_dir.join("00000006-00000010.log").is_file());
    assert!(dep_dir.join("00000011-00000012.log").is_file());

    let mut repo = db.logs();
    let full_log = repo.get_full_log(deployment_id).await.unwrap();
    assert!(full_log.contains("Build step 1"));
    assert!(full_log.contains("Build step 12"));
    assert_eq!(full_log.lines().count(), 12);
}

#[tokio::test]
async fn test_terminal_stream_closure() {
    let db = setup_test_db().await;
    let (_project_id, deployment_id) = create_project_and_deployment(&db).await;

    let mut repo = db.logs();

    // 1. When status is 'running', is_terminal is false
    let res_running = repo
        .query_logs(deployment_id, &LogQuery::default())
        .await
        .unwrap();
    assert!(!res_running.is_terminal);

    // 2. Update status to 'stopped'
    sqlx::query("UPDATE deployments SET status = 'stopped' WHERE id = $1")
        .bind(deployment_id)
        .execute(db.pool())
        .await
        .unwrap();

    // 3. Now is_terminal is true
    let res_stopped = repo
        .query_logs(deployment_id, &LogQuery::default())
        .await
        .unwrap();
    assert!(res_stopped.is_terminal);
}

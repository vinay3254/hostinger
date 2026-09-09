use deploy_platform::{
    artifacts::ArtifactStore,
    cache::{BuildCacheInput, BuildCacheRepository, CacheKey, StoreCacheEntryInput},
    db::Database,
};
use std::fs;
use tempfile::tempdir;
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

#[test]
fn cache_key_stable_ordering_and_canonicalization() {
    let input1 = BuildCacheInput {
        schema_version: "v1".into(),
        framework: "static".into(),
        toolchain: "rust-1.80".into(),
        build_command: "npm run build".into(),
        lockfile_digest: "sha256-abc123456789".into(),
        declared_env_keys: vec!["PUBLIC_URL".into(), "API_ENDPOINT".into(), "APP_ENV".into()],
    };

    // input2 has the exact same declared env keys but in different order
    let input2 = BuildCacheInput {
        schema_version: "v1".into(),
        framework: "static".into(),
        toolchain: "rust-1.80".into(),
        build_command: "npm run build".into(),
        lockfile_digest: "sha256-abc123456789".into(),
        declared_env_keys: vec!["APP_ENV".into(), "PUBLIC_URL".into(), "API_ENDPOINT".into()],
    };

    let key1 = CacheKey::from_build_inputs(&input1);
    let key2 = CacheKey::from_build_inputs(&input2);

    assert_eq!(key1.as_str(), key2.as_str());
    assert_eq!(key1.digest(), key2.digest());
}

#[test]
fn cache_key_changes_on_input_difference() {
    let base = BuildCacheInput {
        schema_version: "v1".into(),
        framework: "static".into(),
        toolchain: "rust-1.80".into(),
        build_command: "npm run build".into(),
        lockfile_digest: "sha256-lockfile".into(),
        declared_env_keys: vec!["APP_ENV".into()],
    };
    let base_key = CacheKey::from_build_inputs(&base);

    // 1. Different lockfile
    let mut diff_lock = base.clone();
    diff_lock.lockfile_digest = "sha256-lockfile-modified".into();
    assert_ne!(
        base_key.digest(),
        CacheKey::from_build_inputs(&diff_lock).digest()
    );

    // 2. Different build command
    let mut diff_cmd = base.clone();
    diff_cmd.build_command = "npm run build:prod".into();
    assert_ne!(
        base_key.digest(),
        CacheKey::from_build_inputs(&diff_cmd).digest()
    );

    // 3. Different toolchain
    let mut diff_tc = base.clone();
    diff_tc.toolchain = "rust-1.81".into();
    assert_ne!(
        base_key.digest(),
        CacheKey::from_build_inputs(&diff_tc).digest()
    );

    // 4. Different framework
    let mut diff_fw = base.clone();
    diff_fw.framework = "nextjs".into();
    assert_ne!(
        base_key.digest(),
        CacheKey::from_build_inputs(&diff_fw).digest()
    );

    // 5. Different env keys
    let mut diff_env = base.clone();
    diff_env.declared_env_keys = vec!["APP_ENV".into(), "DEBUG".into()];
    assert_ne!(
        base_key.digest(),
        CacheKey::from_build_inputs(&diff_env).digest()
    );

    // 6. Schema version change
    let mut diff_schema = base.clone();
    diff_schema.schema_version = "v2".into();
    assert_ne!(
        base_key.digest(),
        CacheKey::from_build_inputs(&diff_schema).digest()
    );
}

#[test]
fn cache_key_never_contains_secret_values() {
    let secret = "ghp_super_secret_token_123456789";
    let input = BuildCacheInput {
        schema_version: "v1".into(),
        framework: "static".into(),
        toolchain: "rust-1.80".into(),
        build_command: "cargo build".into(),
        lockfile_digest: "sha256-abc".into(),
        declared_env_keys: vec!["GITHUB_TOKEN".into()],
    };

    let key = CacheKey::from_build_inputs(&input);
    let canonical = key.canonical_json();

    assert!(!canonical.contains(secret));
    assert!(!key.as_str().contains(secret));
}

#[test]
fn artifact_store_atomic_upload_and_checksum_verification() {
    let temp = tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());

    let dep_id = Uuid::new_v4();
    let sample_file = temp.path().join("source.tar.gz");
    fs::write(&sample_file, b"content-of-build-artifact").unwrap();

    // Store artifact atomically
    let meta = store.store_artifact(dep_id, &sample_file).unwrap();
    assert_eq!(meta.size_bytes, 25);
    assert!(!meta.sha256.is_empty());

    // Verify valid checksum succeeds
    assert!(store.verify_checksum(&meta.file_path, &meta.sha256).is_ok());

    // Verify invalid checksum returns error
    let bad_checksum = "0000000000000000000000000000000000000000000000000000000000000000";
    assert!(store
        .verify_checksum(&meta.file_path, bad_checksum)
        .is_err());
}

#[tokio::test]
async fn build_cache_repository_crud_and_invalidation() {
    let db = setup_test_db().await;
    let user_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();

    // Create user and project in DB
    sqlx::query("INSERT INTO users (id, email, password_hash, name, created_at) VALUES ($1, $2, 'hash', 'Test', NOW())")
        .bind(user_id)
        .bind(format!("user-cache-{}@example.com", user_id))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query("INSERT INTO projects (id, user_id, name, source_dir, base_image, server_command, created_at) VALUES ($1, $2, $3, '/tmp', 'alpine', ARRAY['echo'], NOW())")
        .bind(project_id)
        .bind(user_id)
        .bind(format!("proj-cache-{}", project_id))
        .execute(db.pool())
        .await
        .unwrap();

    let mut repo = BuildCacheRepository::new(db.pool().into());
    let cache_key = format!("cache_key_{}", Uuid::new_v4());

    let entry_input = StoreCacheEntryInput {
        cache_key: cache_key.clone(),
        project_id,
        artifact_checksum: "sha256-valid-sum".into(),
        size_bytes: 1024,
        storage_path: "/tmp/artifacts/cache.tar.gz".into(),
        toolchain: "rust-1.80".into(),
    };

    // 1. Store cache entry
    let entry = repo.store(&entry_input).await.unwrap();
    assert_eq!(entry.cache_key, cache_key);
    assert!(!entry.is_invalidated);

    // 2. Fetch valid cache entry -> Found
    let valid = repo.get_valid(project_id, &cache_key).await.unwrap();
    assert!(valid.is_some());
    assert_eq!(valid.unwrap().id, entry.id);

    // 3. Invalidate specific cache key
    repo.invalidate(project_id, &cache_key).await.unwrap();

    // 4. Fetch valid cache entry -> None (invalidated)
    let after_inv = repo.get_valid(project_id, &cache_key).await.unwrap();
    assert!(after_inv.is_none());

    // 5. Store another cache entry for the project
    let cache_key_2 = format!("cache_key_{}", Uuid::new_v4());
    repo.store(&StoreCacheEntryInput {
        cache_key: cache_key_2.clone(),
        project_id,
        artifact_checksum: "sha256-valid-sum-2".into(),
        size_bytes: 2048,
        storage_path: "/tmp/artifacts/cache2.tar.gz".into(),
        toolchain: "rust-1.80".into(),
    })
    .await
    .unwrap();

    assert!(repo
        .get_valid(project_id, &cache_key_2)
        .await
        .unwrap()
        .is_some());

    // 6. Invalidate all cache entries for project
    repo.invalidate_project(project_id).await.unwrap();
    assert!(repo
        .get_valid(project_id, &cache_key_2)
        .await
        .unwrap()
        .is_none());
}

use deploy_platform::{
    artifacts::ArtifactStore, build_executor::MinidockBuildExecutor, db::Database,
    queue::BuildQueue, worker::BuildWorker,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/deploy_platform".into());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let worker_id = std::env::var("WORKER_ID")
        .unwrap_or_else(|_| format!("worker-{}", Uuid::new_v4().simple()));
    let worker_group = std::env::var("WORKER_GROUP").unwrap_or_else(|_| "build_workers".into());
    let workspace_root = PathBuf::from(
        std::env::var("WORKER_WORKSPACE")
            .unwrap_or_else(|_| "/tmp/deploy-platform/worker-workspace".into()),
    );
    let artifacts_dir = PathBuf::from(
        std::env::var("ARTIFACTS_DIR").unwrap_or_else(|_| "/tmp/deploy-platform/artifacts".into()),
    );

    println!("[build-worker] Starting worker {worker_id} (group: {worker_group})...");

    let db = Database::connect(&database_url).await?;
    db.migrate().await?;
    println!("[build-worker] Database connected and migrated.");

    let redis_client = redis::Client::open(redis_url)?;
    println!("[build-worker] Redis connected.");

    let queue = BuildQueue::new(db.clone(), redis_client, worker_group).await?;
    let artifact_store = ArtifactStore::new(artifacts_dir.clone());
    let executor = Arc::new(MinidockBuildExecutor::new(artifacts_dir));

    let worker = BuildWorker::new(
        worker_id.clone(),
        db.pool().clone(),
        queue,
        executor,
        artifact_store,
        workspace_root,
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Setup SIGINT & SIGTERM handlers
    tokio::spawn(async move {
        let mut sigterm = signal(SignalKind::terminate()).expect("failed to bind SIGTERM listener");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("[build-worker] Received SIGINT (Ctrl-C), initiating graceful shutdown...");
            }
            _ = sigterm.recv() => {
                println!("[build-worker] Received SIGTERM, initiating graceful shutdown...");
            }
        }
        let _ = shutdown_tx.send(true);
    });

    println!("[build-worker] Worker {worker_id} is running and listening for jobs.");
    worker.run_loop(shutdown_rx).await?;
    println!("[build-worker] Worker {worker_id} shutdown gracefully.");

    Ok(())
}

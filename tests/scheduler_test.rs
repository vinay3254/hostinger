use deploy_platform::agent_protocol::{NodeCapacity, RegisterNodeCommand};
use deploy_platform::db::Database;
use deploy_platform::node_registry::NodeRegistry;
use deploy_platform::scheduler::{PlacementRequest, RescheduleRequest, Scheduler, SchedulerError};
use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;

async fn setup_test_pool() -> sqlx::PgPool {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/deploy_platform".to_string()
    });
    let db = Database::connect(&database_url)
        .await
        .expect("database connect failed");
    db.migrate().await.expect("database migration failed");
    db.pool().clone()
}

#[tokio::test]
async fn test_bin_packing_least_loaded_selection() {
    let pool = setup_test_pool().await;
    let registry = NodeRegistry::new();
    let scheduler = Scheduler::new();

    // Node 1: 50% loaded
    let node1_id = Uuid::new_v4();
    registry
        .register_node(
            &pool,
            &RegisterNodeCommand {
                node_id: node1_id,
                hostname: "node-heavy".to_string(),
                endpoint: "http://10.0.1.1:9090".to_string(),
                capacity: NodeCapacity {
                    cpu_millicores: 4000,
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    max_releases: 10,
                },
                token_hash: "hash1".to_string(),
            },
        )
        .await
        .expect("reg 1");

    sqlx::query(
        "UPDATE nodes SET cpu_used_millicores = 2000, memory_used_bytes = 4294967296 WHERE id = $1",
    )
    .bind(node1_id)
    .execute(&pool)
    .await
    .expect("update node1 load");

    // Node 2: 10% loaded
    let node2_id = Uuid::new_v4();
    registry
        .register_node(
            &pool,
            &RegisterNodeCommand {
                node_id: node2_id,
                hostname: "node-light".to_string(),
                endpoint: "http://10.0.1.2:9090".to_string(),
                capacity: NodeCapacity {
                    cpu_millicores: 4000,
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    max_releases: 10,
                },
                token_hash: "hash2".to_string(),
            },
        )
        .await
        .expect("reg 2");

    sqlx::query(
        "UPDATE nodes SET cpu_used_millicores = 400, memory_used_bytes = 858993459 WHERE id = $1",
    )
    .bind(node2_id)
    .execute(&pool)
    .await
    .expect("update node2 load");

    let req = PlacementRequest {
        release_id: Uuid::new_v4(),
        deployment_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        required_cpu_millicores: 500,
        required_memory_bytes: 1024 * 1024 * 1024,
        candidate_nodes: Some(HashSet::from([node1_id, node2_id])),
        anti_affinity_nodes: HashSet::new(),
    };

    let decision = scheduler
        .schedule_release(&pool, &req, Duration::from_secs(30))
        .await
        .expect("schedule release");

    // Least loaded node2 must be selected
    assert_eq!(decision.selected_node_id, node2_id);
    assert_eq!(decision.allocated_cpu_millicores, 500);
}

#[tokio::test]
async fn test_capacity_rejection_when_insufficient_resources() {
    let pool = setup_test_pool().await;
    let scheduler = Scheduler::new();

    // Request huge resource amount (128 CPUs, 1TB RAM)
    let req = PlacementRequest {
        release_id: Uuid::new_v4(),
        deployment_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        required_cpu_millicores: 128_000,
        required_memory_bytes: 1024 * 1024 * 1024 * 1024,
        candidate_nodes: None,
        anti_affinity_nodes: HashSet::new(),
    };

    let result = scheduler
        .schedule_release(&pool, &req, Duration::from_secs(30))
        .await;
    assert!(matches!(result, Err(SchedulerError::InsufficientCapacity)));
}

#[tokio::test]
async fn test_anti_affinity_and_draining_exclusion() {
    let pool = setup_test_pool().await;
    let registry = NodeRegistry::new();
    let scheduler = Scheduler::new();

    let node_a = Uuid::new_v4();
    registry
        .register_node(
            &pool,
            &RegisterNodeCommand {
                node_id: node_a,
                hostname: "node-a".to_string(),
                endpoint: "http://10.0.2.1:9090".to_string(),
                capacity: NodeCapacity {
                    cpu_millicores: 4000,
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    max_releases: 10,
                },
                token_hash: "hash_a".to_string(),
            },
        )
        .await
        .expect("reg a");

    let node_b = Uuid::new_v4();
    registry
        .register_node(
            &pool,
            &RegisterNodeCommand {
                node_id: node_b,
                hostname: "node-b".to_string(),
                endpoint: "http://10.0.2.2:9090".to_string(),
                capacity: NodeCapacity {
                    cpu_millicores: 4000,
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    max_releases: 10,
                },
                token_hash: "hash_b".to_string(),
            },
        )
        .await
        .expect("reg b");

    // Exclude node_b via anti-affinity
    let mut anti_affinity = HashSet::new();
    anti_affinity.insert(node_b);

    let req = PlacementRequest {
        release_id: Uuid::new_v4(),
        deployment_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        required_cpu_millicores: 500,
        required_memory_bytes: 512 * 1024 * 1024,
        candidate_nodes: Some(HashSet::from([node_a, node_b])),
        anti_affinity_nodes: anti_affinity,
    };

    let decision = scheduler
        .schedule_release(&pool, &req, Duration::from_secs(30))
        .await
        .expect("schedule");
    assert_eq!(decision.selected_node_id, node_a);

    // Now drain node_a
    registry
        .set_drain(&pool, node_a, true, Some("drain node a"))
        .await
        .expect("drain a");

    // With node_a draining and node_b anti-affinity, no eligible node
    let mut anti_affinity2 = HashSet::new();
    anti_affinity2.insert(node_b);
    let req2 = PlacementRequest {
        release_id: Uuid::new_v4(),
        deployment_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        required_cpu_millicores: 500,
        required_memory_bytes: 512 * 1024 * 1024,
        candidate_nodes: Some(HashSet::from([node_a, node_b])),
        anti_affinity_nodes: anti_affinity2,
    };

    let res2 = scheduler
        .schedule_release(&pool, &req2, Duration::from_secs(30))
        .await;
    assert!(matches!(res2, Err(SchedulerError::InsufficientCapacity)));
}

#[tokio::test]
async fn test_lease_expiry_and_rescheduling() {
    let pool = setup_test_pool().await;
    let registry = NodeRegistry::new();
    let scheduler = Scheduler::new();

    let node1 = Uuid::new_v4();
    registry
        .register_node(
            &pool,
            &RegisterNodeCommand {
                node_id: node1,
                hostname: "node-lease-1".to_string(),
                endpoint: "http://10.0.3.1:9090".to_string(),
                capacity: NodeCapacity {
                    cpu_millicores: 4000,
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    max_releases: 10,
                },
                token_hash: "hash_l1".to_string(),
            },
        )
        .await
        .expect("reg l1");

    let node2 = Uuid::new_v4();
    registry
        .register_node(
            &pool,
            &RegisterNodeCommand {
                node_id: node2,
                hostname: "node-lease-2".to_string(),
                endpoint: "http://10.0.3.2:9090".to_string(),
                capacity: NodeCapacity {
                    cpu_millicores: 4000,
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    max_releases: 10,
                },
                token_hash: "hash_l2".to_string(),
            },
        )
        .await
        .expect("reg l2");

    let release_id = Uuid::new_v4();
    let req = PlacementRequest {
        release_id,
        deployment_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        required_cpu_millicores: 1000,
        required_memory_bytes: 1024 * 1024 * 1024,
        candidate_nodes: Some(HashSet::from([node1, node2])),
        anti_affinity_nodes: HashSet::new(),
    };

    let decision = scheduler
        .schedule_release(&pool, &req, Duration::from_secs(30))
        .await
        .expect("schedule");

    // Manually age the lease to expire it
    sqlx::query(
        "UPDATE placements SET lease_expires_at = NOW() - INTERVAL '5 seconds' WHERE release_id = $1",
    )
    .bind(release_id)
    .execute(&pool)
    .await
    .expect("expire lease");

    // Reconcile expired leases
    let expired_releases = scheduler
        .reconcile_expired_leases(&pool)
        .await
        .expect("reconcile");
    assert!(expired_releases.contains(&release_id));

    // Reschedule to alternate node with anti-affinity against failed node
    let rescheduled = scheduler
        .reschedule_failed_release(
            &pool,
            RescheduleRequest {
                release_id,
                failed_node_id: decision.selected_node_id,
                required_cpu_millicores: 1000,
                required_memory_bytes: 1024 * 1024 * 1024,
                candidate_nodes: Some(HashSet::from([node1, node2])),
                lease_duration: Duration::from_secs(30),
            },
        )
        .await
        .expect("reschedule");

    assert_ne!(rescheduled.selected_node_id, decision.selected_node_id);
}

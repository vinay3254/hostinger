use deploy_platform::agent_protocol::{
    HeartbeatCommand, NodeCapacity, NodeStatus, RegisterNodeCommand,
};
use deploy_platform::db::Database;
use deploy_platform::node_registry::{NodeRegistry, NodeRegistryError};
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
async fn test_register_node_and_capacity_validation() {
    let pool = setup_test_pool().await;
    let registry = NodeRegistry::new();

    let node_id = Uuid::new_v4();
    let valid_cmd = RegisterNodeCommand {
        node_id,
        hostname: format!("worker-node-{}", &node_id.to_string()[0..8]),
        endpoint: "http://10.0.0.10:9090".to_string(),
        capacity: NodeCapacity {
            cpu_millicores: 4000,
            memory_bytes: 8 * 1024 * 1024 * 1024,
            max_releases: 20,
        },
        token_hash: "hash_secret_token_123".to_string(),
    };

    let reg_res = registry.register_node(&pool, &valid_cmd).await;
    assert!(reg_res.is_ok(), "valid registration must succeed");
    let reg_info = reg_res.unwrap();
    assert!(reg_info.registered);
    assert_eq!(reg_info.assigned_epoch, 1);

    // Verify lookup
    let node = registry
        .get_node(&pool, node_id)
        .await
        .expect("query node")
        .expect("node exists");
    assert_eq!(node.id, node_id);
    assert_eq!(node.hostname, valid_cmd.hostname);
    assert_eq!(node.capacity.cpu_millicores, 4000);
    assert_eq!(node.status, NodeStatus::Online);

    // Reject zero capacity
    let invalid_cmd = RegisterNodeCommand {
        node_id: Uuid::new_v4(),
        hostname: "zero-node".to_string(),
        endpoint: "http://10.0.0.11:9090".to_string(),
        capacity: NodeCapacity {
            cpu_millicores: 0,
            memory_bytes: 0,
            max_releases: 0,
        },
        token_hash: "dummy_hash".to_string(),
    };

    let err = registry.register_node(&pool, &invalid_cmd).await;
    assert!(matches!(err, Err(NodeRegistryError::InvalidCapacity(_))));
}

#[tokio::test]
async fn test_heartbeat_update_and_utilization_tracking() {
    let pool = setup_test_pool().await;
    let registry = NodeRegistry::new();

    let node_id = Uuid::new_v4();
    let reg_cmd = RegisterNodeCommand {
        node_id,
        hostname: format!("worker-{}", &node_id.to_string()[0..8]),
        endpoint: "http://10.0.0.12:9090".to_string(),
        capacity: NodeCapacity {
            cpu_millicores: 8000,
            memory_bytes: 16 * 1024 * 1024 * 1024,
            max_releases: 30,
        },
        token_hash: "hash_secret_token_456".to_string(),
    };
    registry
        .register_node(&pool, &reg_cmd)
        .await
        .expect("registration");

    let r1 = Uuid::new_v4();
    let r2 = Uuid::new_v4();
    let heartbeat = HeartbeatCommand {
        node_id,
        epoch: 1,
        cpu_used_millicores: 1500,
        memory_used_bytes: 2 * 1024 * 1024 * 1024,
        running_releases: vec![r1, r2],
        status: NodeStatus::Online,
    };

    let hb_resp = registry
        .record_heartbeat(&pool, &heartbeat)
        .await
        .expect("heartbeat");
    assert!(hb_resp.acknowledged);
    assert!(!hb_resp.drain_requested);

    let updated_node = registry
        .get_node(&pool, node_id)
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(updated_node.cpu_used_millicores, 1500);
    assert_eq!(updated_node.memory_used_bytes, 2 * 1024 * 1024 * 1024);
    assert_eq!(updated_node.running_releases_count, 2);
    assert!(updated_node.version > 1);
}

#[tokio::test]
async fn test_drain_and_disable_controls() {
    let pool = setup_test_pool().await;
    let registry = NodeRegistry::new();

    let node_id = Uuid::new_v4();
    let reg_cmd = RegisterNodeCommand {
        node_id,
        hostname: format!("drain-worker-{}", &node_id.to_string()[0..8]),
        endpoint: "http://10.0.0.13:9090".to_string(),
        capacity: NodeCapacity {
            cpu_millicores: 4000,
            memory_bytes: 8 * 1024 * 1024 * 1024,
            max_releases: 10,
        },
        token_hash: "hash_drain_token".to_string(),
    };
    registry
        .register_node(&pool, &reg_cmd)
        .await
        .expect("registration");

    // Initially node is in active pool
    let active_nodes = registry
        .list_active_nodes(&pool, Duration::from_secs(30))
        .await
        .expect("list active");
    assert!(active_nodes.iter().any(|n| n.id == node_id));

    // Enable drain
    registry
        .set_drain(&pool, node_id, true, Some("Scheduled maintenance"))
        .await
        .expect("set drain");

    let drained_node = registry
        .get_node(&pool, node_id)
        .await
        .expect("query")
        .expect("exists");
    assert!(drained_node.is_draining);
    assert_eq!(drained_node.status, NodeStatus::Draining);

    // Heartbeat receives drain_requested = true
    let heartbeat = HeartbeatCommand {
        node_id,
        epoch: 1,
        cpu_used_millicores: 500,
        memory_used_bytes: 1024 * 1024 * 1024,
        running_releases: vec![],
        status: NodeStatus::Draining,
    };
    let hb_resp = registry
        .record_heartbeat(&pool, &heartbeat)
        .await
        .expect("heartbeat");
    assert!(hb_resp.drain_requested);

    // Draining node excluded from active placement pool
    let active_nodes_after = registry
        .list_active_nodes(&pool, Duration::from_secs(30))
        .await
        .expect("list active");
    assert!(!active_nodes_after.iter().any(|n| n.id == node_id));

    // Disable node completely
    registry
        .set_enabled(&pool, node_id, false, Some("Hardware check"))
        .await
        .expect("disable node");
    let disabled_node = registry
        .get_node(&pool, node_id)
        .await
        .expect("query")
        .expect("exists");
    assert!(!disabled_node.is_enabled);
}

#[tokio::test]
async fn test_stale_heartbeat_and_expiry_evaluation() {
    let pool = setup_test_pool().await;
    let registry = NodeRegistry::new();

    let node_id = Uuid::new_v4();
    let reg_cmd = RegisterNodeCommand {
        node_id,
        hostname: format!("stale-worker-{}", &node_id.to_string()[0..8]),
        endpoint: "http://10.0.0.14:9090".to_string(),
        capacity: NodeCapacity {
            cpu_millicores: 4000,
            memory_bytes: 8 * 1024 * 1024 * 1024,
            max_releases: 10,
        },
        token_hash: "hash_stale_token".to_string(),
    };
    registry
        .register_node(&pool, &reg_cmd)
        .await
        .expect("registration");

    // Manually age the heartbeat to simulate missing heartbeats
    sqlx::query("UPDATE nodes SET last_heartbeat_at = NOW() - INTERVAL '45 seconds' WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("age node heartbeat");

    // Evaluate health with a 15-second heartbeat threshold
    let transition_count = registry
        .evaluate_health_and_expiry(&pool, Duration::from_secs(15))
        .await
        .expect("evaluate health");
    assert!(transition_count >= 1);

    let degraded_or_offline_node = registry
        .get_node(&pool, node_id)
        .await
        .expect("query")
        .expect("exists");
    assert!(
        degraded_or_offline_node.status == NodeStatus::Degraded
            || degraded_or_offline_node.status == NodeStatus::Offline
    );

    // Events recorded in node_events
    let events = registry
        .get_node_events(&pool, node_id)
        .await
        .expect("events");
    assert!(!events.is_empty());
}

#[tokio::test]
async fn test_node_identity_rotation() {
    let pool = setup_test_pool().await;
    let registry = NodeRegistry::new();

    let node_id = Uuid::new_v4();
    let reg_cmd = RegisterNodeCommand {
        node_id,
        hostname: format!("rotate-worker-{}", &node_id.to_string()[0..8]),
        endpoint: "http://10.0.0.15:9090".to_string(),
        capacity: NodeCapacity {
            cpu_millicores: 4000,
            memory_bytes: 8 * 1024 * 1024 * 1024,
            max_releases: 10,
        },
        token_hash: "old_token_hash".to_string(),
    };
    registry
        .register_node(&pool, &reg_cmd)
        .await
        .expect("registration");

    // Rotate identity
    let rotated_epoch = registry
        .rotate_node_identity(&pool, node_id, "new_token_hash_789")
        .await
        .expect("rotate identity");
    assert_eq!(rotated_epoch, 2);

    let node = registry
        .get_node(&pool, node_id)
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(node.epoch, 2);
    assert_eq!(node.token_hash, "new_token_hash_789");

    // Heartbeat with old epoch 1 is rejected
    let stale_epoch_hb = HeartbeatCommand {
        node_id,
        epoch: 1, // outdated
        cpu_used_millicores: 100,
        memory_used_bytes: 512 * 1024 * 1024,
        running_releases: vec![],
        status: NodeStatus::Online,
    };
    let err = registry.record_heartbeat(&pool, &stale_epoch_hb).await;
    assert!(matches!(
        err,
        Err(NodeRegistryError::EpochMismatch {
            current: 2,
            provided: 1
        })
    ));
}

use deploy_platform::agent_client::AgentClient;
use deploy_platform::agent_protocol::{
    AgentCommand, AgentResponse, CreateReleaseCommand, NodeCredentials, AGENT_PROTOCOL_VERSION,
};
use deploy_platform::agent_server::{AgentServer, MockAgentRuntimeAdapter};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn test_agent_server_lifecycle_and_execution() {
    let node_id = Uuid::new_v4();
    let creds = NodeCredentials::generate(node_id);
    let mock_runtime = Arc::new(MockAgentRuntimeAdapter::new());
    let server = AgentServer::new(node_id, creds.clone(), mock_runtime.clone());

    let release_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();
    let create_cmd = AgentCommand::CreateRelease(CreateReleaseCommand {
        release_id,
        deployment_id: dep_id,
        image_path: "/var/lib/artifacts/app.tar.gz".to_string(),
        env: HashMap::from([("PORT".to_string(), "8080".to_string())]),
        cpu_limit_millicores: 2000,
        memory_limit_bytes: 1024 * 1024 * 1024,
    });

    let resp = server
        .execute_command_direct(create_cmd)
        .await
        .expect("create release");
    let container_id = match resp {
        AgentResponse::CreateRelease(res) => {
            assert_eq!(res.release_id, release_id);
            assert!(!res.container_id.is_empty());
            res.container_id
        }
        other => panic!("expected CreateRelease response, got {:?}", other),
    };

    // Start release
    let start_cmd = AgentCommand::StartRelease {
        release_id,
        container_id: container_id.clone(),
    };
    let start_resp = server
        .execute_command_direct(start_cmd)
        .await
        .expect("start");
    assert!(matches!(start_resp, AgentResponse::StartRelease(r) if r.started));

    // Stats
    let stats_cmd = AgentCommand::ReleaseStats { release_id };
    let stats_resp = server
        .execute_command_direct(stats_cmd)
        .await
        .expect("stats");
    assert!(matches!(stats_resp, AgentResponse::ReleaseStats(r) if r.cpu_millicores > 0));

    // Logs
    let logs_cmd = AgentCommand::ReleaseLogs {
        release_id,
        since_seq: None,
        limit: 10,
    };
    let logs_resp = server.execute_command_direct(logs_cmd).await.expect("logs");
    assert!(matches!(logs_resp, AgentResponse::ReleaseLogs(r) if !r.logs.is_empty()));

    // Stop release
    let stop_cmd = AgentCommand::StopRelease {
        release_id,
        container_id,
        grace_period_secs: 5,
    };
    let stop_resp = server.execute_command_direct(stop_cmd).await.expect("stop");
    assert!(matches!(stop_resp, AgentResponse::StopRelease(r) if r.stopped));
}

#[tokio::test]
async fn test_agent_server_rejection_when_draining() {
    let node_id = Uuid::new_v4();
    let creds = NodeCredentials::generate(node_id);
    let mock_runtime = Arc::new(MockAgentRuntimeAdapter::new());
    let server = AgentServer::new(node_id, creds.clone(), mock_runtime.clone());

    // Mark server draining
    server.set_draining(true);
    assert!(server.is_draining());

    // New create release must be rejected
    let create_cmd = AgentCommand::CreateRelease(CreateReleaseCommand {
        release_id: Uuid::new_v4(),
        deployment_id: Uuid::new_v4(),
        image_path: "/var/lib/artifacts/app.tar.gz".to_string(),
        env: HashMap::new(),
        cpu_limit_millicores: 1000,
        memory_limit_bytes: 512 * 1024 * 1024,
    });

    let result = server.execute_command_direct(create_cmd).await;
    assert!(
        result.is_err(),
        "create release must be rejected when draining"
    );
}

#[tokio::test]
async fn test_agent_client_to_server_http_roundtrip() {
    let node_id = Uuid::new_v4();
    let creds = NodeCredentials::generate(node_id);
    let mock_runtime = Arc::new(MockAgentRuntimeAdapter::new());
    let server = Arc::new(AgentServer::new(
        node_id,
        creds.clone(),
        mock_runtime.clone(),
    ));

    // Bind local test listener
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let local_addr = listener.local_addr().expect("local addr");
    let app = AgentServer::into_router(server);

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server serve");
    });

    // Create client
    let client = AgentClient::new(
        node_id,
        format!("http://{}", local_addr),
        creds.secret_key.clone(),
    );

    // Health check via client
    let health_resp = client
        .execute(AgentCommand::Health)
        .await
        .expect("health rpc");
    match health_resp {
        AgentResponse::Health(h) => {
            assert!(h.healthy);
            assert_eq!(h.version, AGENT_PROTOCOL_VERSION);
        }
        other => panic!("expected Health response, got {:?}", other),
    }

    // Unauthorized client with wrong secret key
    let bad_client = AgentClient::new(
        node_id,
        format!("http://{}", local_addr),
        "wrong_secret_key_1234567890abcdef".to_string(),
    );
    let bad_result = bad_client.execute(AgentCommand::Health).await;
    assert!(bad_result.is_err(), "bad signature must fail");
}

use clap::Parser;
use deploy_platform::agent_protocol::NodeCredentials;
use deploy_platform::agent_server::{AgentServer, MockAgentRuntimeAdapter};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "platform-agent", about = "Deploy Platform Node Agent")]
struct Args {
    #[arg(long)]
    node_id: Option<Uuid>,

    #[arg(long)]
    secret_key: Option<String>,

    #[arg(long, default_value = "0.0.0.0:9090")]
    listen_addr: SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let node_id = args
        .node_id
        .or_else(|| std::env::var("NODE_ID").ok().and_then(|s| s.parse().ok()))
        .unwrap_or_else(Uuid::new_v4);

    let secret_key = args
        .secret_key
        .or_else(|| std::env::var("NODE_SECRET_KEY").ok())
        .unwrap_or_else(|| "default_secret_key_0000000000000000".to_string());

    println!("Starting deploy-platform node agent for node {}", node_id);

    let creds = NodeCredentials {
        node_id,
        secret_key,
        epoch: 1,
    };

    let runtime = Arc::new(MockAgentRuntimeAdapter::new());
    let server = Arc::new(AgentServer::new(node_id, creds, runtime));

    let app = AgentServer::into_router(server.clone());

    let listener = tokio::net::TcpListener::bind(args.listen_addr).await?;
    println!("Node agent listening on {}", args.listen_addr);

    let server_handle = tokio::spawn(async move { axum::serve(listener, app).await });

    // Wait for shutdown signal
    match signal::ctrl_c().await {
        Ok(()) => {
            println!("Shutdown signal received. Setting node agent to draining state...");
            server.set_draining(true);
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            println!("Node agent gracefully drained.");
        }
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
        }
    }

    server_handle.abort();
    Ok(())
}

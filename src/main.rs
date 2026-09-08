use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use deploy_platform::{
    api,
    builder::MinidockImageBuilder,
    config::PlatformConfig,
    model::CreateProjectInput,
    runtime::MinidockRuntime,
    service::{DeploymentService, PlatformService},
    store::StateStore,
};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "platform", about = "Local single-node static deploy platform")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        listen: Option<String>,
    },
    #[command(subcommand)]
    Project(ProjectCommand),
    Deploy {
        project_id: String,
    },
    Deployments {
        project_id: String,
    },
    Logs {
        deployment_id: String,
    },
    Stop {
        deployment_id: String,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        base_image: PathBuf,
    },
    Show {
        project_id: String,
    },
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = PlatformConfig::from_env().context("failed to load platform configuration")?;

    let store = StateStore::at(config.state_dir.clone());
    let minidock_store = minidock::StateStore::from_current_user()
        .context("failed to initialize minidock state store")?;
    let runtime = MinidockRuntime { minidock_store };
    let builder = MinidockImageBuilder;

    let service: Arc<dyn PlatformService> =
        Arc::new(DeploymentService::new(store, runtime, builder));

    match cli.command {
        Command::Serve { listen } => {
            let addr = listen.unwrap_or(config.listen_addr);
            let listener = tokio::net::TcpListener::bind(&addr)
                .await
                .with_context(|| format!("failed to bind to {addr}"))?;
            axum::serve(listener, api::router(service))
                .await
                .context("api server failed")?;
        }
        Command::Project(ProjectCommand::Create {
            name,
            source,
            base_image,
        }) => {
            let input = CreateProjectInput {
                name,
                source_dir: source,
                base_image,
                server_command: config.server_command,
            };
            let project = service.create_project(input)?;
            println!("{}", serde_json::to_string_pretty(&project)?);
        }
        Command::Project(ProjectCommand::Show { project_id }) => {
            let id = Uuid::parse_str(&project_id)
                .with_context(|| format!("invalid project ID: {project_id}"))?;
            let project = service.project(id)?;
            println!("{}", serde_json::to_string_pretty(&project)?);
        }
        Command::Deploy { project_id } => {
            let id = Uuid::parse_str(&project_id)
                .with_context(|| format!("invalid project ID: {project_id}"))?;
            let deployment = service.deploy(id)?;
            println!("{}", serde_json::to_string_pretty(&deployment)?);
        }
        Command::Deployments { project_id } => {
            let id = Uuid::parse_str(&project_id)
                .with_context(|| format!("invalid project ID: {project_id}"))?;
            let deployments = service.deployments(id)?;
            println!("{}", serde_json::to_string_pretty(&deployments)?);
        }
        Command::Logs { deployment_id } => {
            let id = Uuid::parse_str(&deployment_id)
                .with_context(|| format!("invalid deployment ID: {deployment_id}"))?;
            let logs = service.logs(id)?;
            print!("{logs}");
        }
        Command::Stop { deployment_id } => {
            let id = Uuid::parse_str(&deployment_id)
                .with_context(|| format!("invalid deployment ID: {deployment_id}"))?;
            let deployment = service.stop(id)?;
            println!("{}", serde_json::to_string_pretty(&deployment)?);
        }
    }

    Ok(())
}

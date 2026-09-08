use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformConfig {
    pub state_dir: PathBuf,
    pub listen_addr: String,
    pub server_command: Vec<String>,
}

pub fn parse_server_command(input: &str) -> anyhow::Result<Vec<String>> {
    let tokens: Vec<String> = input.split_ascii_whitespace().map(String::from).collect();
    if tokens.is_empty() {
        anyhow::bail!("server command cannot be empty");
    }
    let port_exact = tokens.iter().filter(|t| *t == "{PORT}").count();
    let port_contains = tokens.iter().filter(|t| t.contains("{PORT}")).count();
    if port_exact != 1 || port_contains != 1 {
        anyhow::bail!("server command must contain exactly one {{PORT}} token");
    }
    Ok(tokens)
}

impl PlatformConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let state_dir = match std::env::var("PLATFORM_STATE_DIR") {
            Ok(val) if !val.trim().is_empty() => PathBuf::from(val),
            _ => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".deploy-platform")
            }
        };
        let listen_addr = match std::env::var("PLATFORM_LISTEN_ADDR") {
            Ok(val) if !val.trim().is_empty() => val,
            _ => "127.0.0.1:8787".into(),
        };
        let server_cmd_raw = match std::env::var("PLATFORM_SERVER_COMMAND") {
            Ok(val) if !val.trim().is_empty() => val,
            _ => "/bin/busybox httpd -f -p {PORT} -h /srv/app".into(),
        };
        let server_command = parse_server_command(&server_cmd_raw)?;

        Ok(Self {
            state_dir,
            listen_addr,
            server_command,
        })
    }
}

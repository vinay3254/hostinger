use crate::model::Framework;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildPlan {
    pub framework: Framework,
    pub install: Vec<String>,
    pub build: Vec<String>,
    pub output: PathBuf,
    pub server: Option<Vec<String>>,
}

pub fn detect_build_plan(root: &Path) -> Result<BuildPlan> {
    let dockerfile = root.join("Dockerfile");
    let requirements = root.join("requirements.txt");
    let package_json = root.join("package.json");
    let index_html = root.join("index.html");

    let has_docker = dockerfile.is_file();
    let has_python = requirements.is_file();
    let has_package_json = package_json.is_file();
    let has_index_html = index_html.is_file();

    let mut pkg_value: Option<serde_json::Value> = None;
    if has_package_json {
        let content = fs::read_to_string(&package_json)
            .with_context(|| format!("failed to read {}", package_json.display()))?;
        let parsed: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("invalid package.json in {}: {}", root.display(), e))?;
        pkg_value = Some(parsed);
    }

    let mut detected = Vec::new();
    if has_docker {
        detected.push("docker");
    }
    if has_python {
        detected.push("python");
    }
    if has_package_json {
        detected.push("node");
    }
    if has_index_html && !has_package_json {
        detected.push("static");
    }

    if detected.is_empty() {
        bail!(
            "unsupported framework: no recognized project files found in {}",
            root.display()
        );
    }

    if detected.len() > 1 {
        bail!(
            "ambiguous framework: detected multiple project indicators ({}) in {}",
            detected.join(", "),
            root.display()
        );
    }

    match detected[0] {
        "docker" => Ok(BuildPlan {
            framework: Framework::Docker,
            install: Vec::new(),
            build: vec![
                "docker".into(),
                "build".into(),
                "-t".into(),
                "app".into(),
                ".".into(),
            ],
            output: PathBuf::from("."),
            server: Some(vec!["docker".into(), "run".into(), "app".into()]),
        }),
        "python" => Ok(BuildPlan {
            framework: Framework::Python,
            install: vec![
                "pip".into(),
                "install".into(),
                "-r".into(),
                "requirements.txt".into(),
            ],
            build: Vec::new(),
            output: PathBuf::from("."),
            server: Some(vec!["python3".into(), "app.py".into()]),
        }),
        "node" => {
            let pkg = pkg_value.expect("package.json was already parsed");

            let is_next = pkg
                .get("dependencies")
                .and_then(|d| d.get("next"))
                .is_some()
                || pkg
                    .get("devDependencies")
                    .and_then(|d| d.get("next"))
                    .is_some();

            let (framework, default_output) = if is_next {
                (Framework::NextJs, PathBuf::from(".next"))
            } else {
                (Framework::Node, PathBuf::from("dist"))
            };

            let pm = if root.join("pnpm-lock.yaml").is_file() {
                "pnpm"
            } else if root.join("yarn.lock").is_file() {
                "yarn"
            } else {
                "npm"
            };

            let install = vec![pm.to_string(), "install".to_string()];

            let scripts = pkg.get("scripts");
            let has_build = scripts
                .and_then(|s| s.get("build"))
                .and_then(|b| b.as_str())
                .is_some();

            let build = if has_build {
                if pm == "yarn" {
                    vec!["yarn".into(), "build".into()]
                } else {
                    vec![pm.into(), "run".into(), "build".into()]
                }
            } else {
                Vec::new()
            };

            let has_start = scripts
                .and_then(|s| s.get("start"))
                .and_then(|b| b.as_str())
                .is_some();

            let server = if has_start {
                Some(vec![pm.into(), "start".into()])
            } else {
                None
            };

            Ok(BuildPlan {
                framework,
                install,
                build,
                output: default_output,
                server,
            })
        }
        "static" => Ok(BuildPlan {
            framework: Framework::Static,
            install: Vec::new(),
            build: Vec::new(),
            output: PathBuf::from("."),
            server: Some(vec![
                "python3".into(),
                "-m".into(),
                "http.server".into(),
                "8080".into(),
            ]),
        }),
        _ => unreachable!(),
    }
}

use deploy_platform::{framework::detect_build_plan, model::Framework};
use std::fs;
use tempfile::tempdir;

#[test]
fn detects_static_html_project() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("index.html"), "<h1>Static Site</h1>").unwrap();

    let plan = detect_build_plan(dir.path()).unwrap();
    assert_eq!(plan.framework, Framework::Static);
    assert!(plan.install.is_empty());
    assert!(plan.build.is_empty());
    assert!(plan.server.is_some());
}

#[test]
fn detects_node_project() {
    let dir = tempdir().unwrap();
    let pkg_json = serde_json::json!({
        "name": "node-app",
        "scripts": {
            "build": "tsc",
            "start": "node index.js"
        },
        "dependencies": {
            "express": "^4.18.2"
        }
    });
    fs::write(dir.path().join("package.json"), pkg_json.to_string()).unwrap();

    let plan = detect_build_plan(dir.path()).unwrap();
    assert_eq!(plan.framework, Framework::Node);
    assert_eq!(plan.install, ["npm", "install"]);
    assert_eq!(plan.build, ["npm", "run", "build"]);
    assert_eq!(plan.server, Some(vec!["npm".into(), "start".into()]));
}

#[test]
fn detects_nextjs_project_with_yarn() {
    let dir = tempdir().unwrap();
    let pkg_json = serde_json::json!({
        "name": "next-app",
        "scripts": {
            "build": "next build",
            "start": "next start"
        },
        "dependencies": {
            "next": "^14.0.0",
            "react": "^18.2.0"
        }
    });
    fs::write(dir.path().join("package.json"), pkg_json.to_string()).unwrap();
    fs::write(dir.path().join("yarn.lock"), "").unwrap();

    let plan = detect_build_plan(dir.path()).unwrap();
    assert_eq!(plan.framework, Framework::NextJs);
    assert_eq!(plan.install, ["yarn", "install"]);
    assert_eq!(plan.build, ["yarn", "build"]);
    assert_eq!(plan.server, Some(vec!["yarn".into(), "start".into()]));
}

#[test]
fn detects_python_requirements_project() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("requirements.txt"), "flask==3.0.0\n").unwrap();

    let plan = detect_build_plan(dir.path()).unwrap();
    assert_eq!(plan.framework, Framework::Python);
    assert_eq!(plan.install, ["pip", "install", "-r", "requirements.txt"]);
    assert!(plan.server.is_some());
}

#[test]
fn detects_dockerfile_project() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("Dockerfile"),
        "FROM alpine\nCMD [\"echo\", \"hi\"]\n",
    )
    .unwrap();

    let plan = detect_build_plan(dir.path()).unwrap();
    assert_eq!(plan.framework, Framework::Docker);
}

#[test]
fn rejects_invalid_json_config() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("package.json"), "NOT VALID JSON").unwrap();

    let res = detect_build_plan(dir.path());
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("invalid package.json"));
}

#[test]
fn rejects_ambiguous_project_types() {
    let dir = tempdir().unwrap();
    // Both Python requirements.txt AND package.json
    fs::write(dir.path().join("requirements.txt"), "flask==3.0.0\n").unwrap();
    fs::write(
        dir.path().join("package.json"),
        "{\"name\":\"conflicting\"}",
    )
    .unwrap();

    let res = detect_build_plan(dir.path());
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("ambiguous framework"));
}

#[test]
fn rejects_unsupported_project() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("notes.txt"), "just a readme").unwrap();

    let res = detect_build_plan(dir.path());
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("unsupported framework"));
}

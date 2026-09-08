use deploy_platform::{
    build_executor::{BuildExecutor, MemoryLogSink, MinidockBuildExecutor},
    framework::BuildPlan,
    model::Framework,
};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn test_command_ordering() {
    let ws = tempdir().unwrap();
    let artifacts = tempdir().unwrap();

    let plan = BuildPlan {
        framework: Framework::Static,
        install: vec!["sh".into(), "-c".into(), "echo install >> order.txt".into()],
        build: vec!["sh".into(), "-c".into(), "echo build >> order.txt".into()],
        output: PathBuf::from("."),
        server: None,
    };

    let executor =
        MinidockBuildExecutor::new(artifacts.path()).with_timeout(Duration::from_secs(10));

    let mut log_sink = MemoryLogSink::new();
    let result = executor.execute(&plan, ws.path(), &mut log_sink).unwrap();

    assert!(result.artifact_path.exists());
    let content = fs::read_to_string(ws.path().join("order.txt")).unwrap();
    assert_eq!(content.trim(), "install\nbuild");
}

#[test]
fn test_nonzero_exit_halts_execution() {
    let ws = tempdir().unwrap();
    let artifacts = tempdir().unwrap();

    let plan = BuildPlan {
        framework: Framework::Static,
        install: vec!["sh".into(), "-c".into(), "exit 42".into()],
        build: vec!["touch".into(), "should_not_exist.txt".into()],
        output: PathBuf::from("."),
        server: None,
    };

    let executor = MinidockBuildExecutor::new(artifacts.path());
    let mut log_sink = MemoryLogSink::new();

    let result = executor.execute(&plan, ws.path(), &mut log_sink);
    assert!(result.is_err());
    assert!(!ws.path().join("should_not_exist.txt").exists());
}

#[test]
fn test_timeout_kills_process() {
    let ws = tempdir().unwrap();
    let artifacts = tempdir().unwrap();

    let plan = BuildPlan {
        framework: Framework::Static,
        install: vec!["sleep".into(), "5".into()],
        build: Vec::new(),
        output: PathBuf::from("."),
        server: None,
    };

    let executor =
        MinidockBuildExecutor::new(artifacts.path()).with_timeout(Duration::from_millis(100));

    let mut log_sink = MemoryLogSink::new();
    let result = executor.execute(&plan, ws.path(), &mut log_sink);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("timed out") || err_msg.contains("timeout"),
        "expected timeout error message, got: {err_msg}"
    );
}

#[test]
fn test_log_redaction() {
    let ws = tempdir().unwrap();
    let artifacts = tempdir().unwrap();

    let plan = BuildPlan {
        framework: Framework::Static,
        install: vec![
            "sh".into(),
            "-c".into(),
            "echo super_secret_token_12345 in output".into(),
        ],
        build: Vec::new(),
        output: PathBuf::from("."),
        server: None,
    };

    let executor =
        MinidockBuildExecutor::new(artifacts.path()).with_secret("super_secret_token_12345");

    let mut log_sink = MemoryLogSink::new();
    executor.execute(&plan, ws.path(), &mut log_sink).unwrap();

    let found_redacted = log_sink
        .lines()
        .iter()
        .any(|l| l.message.contains("[REDACTED]"));
    let found_secret = log_sink
        .lines()
        .iter()
        .any(|l| l.message.contains("super_secret_token_12345"));

    assert!(found_redacted, "expected redacted token in logs");
    assert!(!found_secret, "raw secret leaked into logs");
}

#[test]
fn test_rejects_output_outside_workspace() {
    let ws = tempdir().unwrap();
    let artifacts = tempdir().unwrap();

    let plan = BuildPlan {
        framework: Framework::Static,
        install: Vec::new(),
        build: Vec::new(),
        output: PathBuf::from("../../escaped"),
        server: None,
    };

    let executor = MinidockBuildExecutor::new(artifacts.path());
    let mut log_sink = MemoryLogSink::new();

    let result = executor.execute(&plan, ws.path(), &mut log_sink);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("traversal") || err_msg.contains("escape") || err_msg.contains("outside"),
        "expected containment error, got: {err_msg}"
    );
}

#[test]
fn test_packages_artifact_successfully() {
    let ws = tempdir().unwrap();
    let artifacts = tempdir().unwrap();

    fs::write(ws.path().join("index.html"), "<h1>Test</h1>").unwrap();

    let plan = BuildPlan {
        framework: Framework::Static,
        install: Vec::new(),
        build: Vec::new(),
        output: PathBuf::from("."),
        server: None,
    };

    let executor = MinidockBuildExecutor::new(artifacts.path());
    let mut log_sink = MemoryLogSink::new();

    let result = executor.execute(&plan, ws.path(), &mut log_sink).unwrap();
    assert!(result.artifact_path.exists());
    assert!(!result.cache_key.is_empty());
    assert!(result.duration > Duration::ZERO);
}

#[test]
#[ignore]
fn test_privileged_minidock_smoke() {
    let base_image_var = std::env::var("PLATFORM_BASE_IMAGE").unwrap();
    let ws = tempdir().unwrap();
    let artifacts = tempdir().unwrap();

    fs::write(ws.path().join("index.html"), "<h1>Smoke</h1>").unwrap();

    let plan = BuildPlan {
        framework: Framework::Static,
        install: Vec::new(),
        build: Vec::new(),
        output: PathBuf::from("."),
        server: None,
    };

    let executor = MinidockBuildExecutor::new(artifacts.path())
        .with_base_rootfs(PathBuf::from(base_image_var));

    let mut log_sink = MemoryLogSink::new();
    let result = executor.execute(&plan, ws.path(), &mut log_sink).unwrap();
    assert!(result.artifact_path.exists());
}

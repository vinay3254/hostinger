use deploy_platform::{
    providers::{Provider, RepositoryRef},
    source_checkout::checkout_commit,
};
use std::fs;
use std::process::Command;
use tempfile::tempdir;

fn setup_local_git_repo() -> (tempfile::TempDir, String) {
    let dir = tempdir().unwrap();
    let repo_path = dir.path();

    Command::new("git")
        .args(["init"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo_path)
        .output()
        .unwrap();

    fs::write(repo_path.join("index.html"), "<h1>Hello Test</h1>").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(repo_path)
        .output()
        .unwrap();

    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    let commit_sha = String::from_utf8(output.stdout).unwrap().trim().to_string();

    (dir, commit_sha)
}

#[test]
fn checkouts_exact_commit_sha() {
    let (repo_dir, commit_sha) = setup_local_git_repo();
    let workspace = tempdir().unwrap();

    let repo_ref = RepositoryRef {
        provider: Provider::GitHub,
        external_id: "123".into(),
        clone_url: url::Url::from_directory_path(repo_dir.path()).unwrap(),
        default_branch: "main".into(),
    };

    let checkout = checkout_commit(&repo_ref, &commit_sha, workspace.path()).unwrap();
    assert_eq!(checkout.commit_sha, commit_sha);
    assert!(checkout.directory.join("index.html").exists());
}

#[test]
fn rejects_invalid_or_mismatched_sha() {
    let (repo_dir, _) = setup_local_git_repo();
    let workspace = tempdir().unwrap();

    let repo_ref = RepositoryRef {
        provider: Provider::GitHub,
        external_id: "123".into(),
        clone_url: url::Url::from_directory_path(repo_dir.path()).unwrap(),
        default_branch: "main".into(),
    };

    let invalid_sha = "0000000000000000000000000000000000000000";
    let res = checkout_commit(&repo_ref, invalid_sha, workspace.path());
    assert!(res.is_err());
}

#[test]
fn rejects_workspace_traversal_attempts() {
    let (repo_dir, _commit_sha) = setup_local_git_repo();
    let workspace = tempdir().unwrap();

    let repo_ref = RepositoryRef {
        provider: Provider::GitHub,
        external_id: "123".into(),
        clone_url: url::Url::from_directory_path(repo_dir.path()).unwrap(),
        default_branch: "main".into(),
    };

    // Traversal in commit string
    let bad_commit = "../../../etc/passwd";
    let res = checkout_commit(&repo_ref, bad_commit, workspace.path());
    assert!(res.is_err());
}

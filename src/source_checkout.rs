use crate::providers::RepositoryRef;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    pub directory: PathBuf,
    pub commit_sha: String,
}

pub fn checkout_commit(source: &RepositoryRef, commit: &str, workspace: &Path) -> Result<Checkout> {
    let commit = commit.trim();
    if commit.len() < 7 || commit.len() > 64 || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid commit sha: {commit}");
    }

    let checkout_dir = workspace.join("source");
    if !checkout_dir.starts_with(workspace) {
        bail!("path traversal detected in workspace");
    }

    std::fs::create_dir_all(&checkout_dir)
        .with_context(|| format!("failed to create directory {:?}", checkout_dir))?;

    let clone_url_str = if source.clone_url.scheme() == "file" {
        source
            .clone_url
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("invalid file url"))?
            .to_string_lossy()
            .to_string()
    } else {
        source.clone_url.to_string()
    };

    // 1. git init
    run_git_cmd(&checkout_dir, &["init"])?;

    // 2. git remote add origin <url>
    let _ = run_git_cmd(&checkout_dir, &["remote", "add", "origin", &clone_url_str]);

    // 3. git fetch origin <commit> (or fetch origin)
    if run_git_cmd(&checkout_dir, &["fetch", "--depth", "1", "origin", commit]).is_err() {
        run_git_cmd(&checkout_dir, &["fetch", "origin"])?;
    }

    // 4. git checkout <commit>
    run_git_cmd(&checkout_dir, &["checkout", "--detach", commit])?;

    // 5. Verify rev-parse HEAD matches requested commit prefix
    let head_sha = run_git_output(&checkout_dir, &["rev-parse", "HEAD"])?;
    let head_sha = head_sha.trim();
    if !head_sha.starts_with(commit) && !commit.starts_with(head_sha) {
        bail!("checked out commit {head_sha} does not match requested commit {commit}");
    }

    Ok(Checkout {
        directory: checkout_dir,
        commit_sha: head_sha.to_string(),
    })
}

fn run_git_cmd(cwd: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to execute git command: {:?}", args))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git command {:?} failed with status {}: {}",
            args,
            output.status,
            stderr
        );
    }
    Ok(())
}

fn run_git_output(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to execute git command: {:?}", args))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git command {:?} failed with status {}: {}",
            args,
            output.status,
            stderr
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

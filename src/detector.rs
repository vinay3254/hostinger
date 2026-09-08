use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub relative: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticSource {
    pub root: PathBuf,
    pub files: Vec<SourceFile>,
}

pub fn detect_static_source(source_dir: &Path) -> Result<StaticSource> {
    let canonical = source_dir
        .canonicalize()
        .with_context(|| format!("source directory does not exist: {}", source_dir.display()))?;

    if !canonical.is_dir() {
        anyhow::bail!("source path is not a directory: {}", canonical.display());
    }

    let index_html_path = canonical.join("index.html");
    let index_meta = fs::symlink_metadata(&index_html_path).with_context(|| {
        format!(
            "source directory does not contain a root index.html: {}",
            canonical.display()
        )
    })?;

    if index_meta.file_type().is_symlink() {
        anyhow::bail!(
            "root index.html cannot be a symlink: {}",
            index_html_path.display()
        );
    }

    if !index_meta.is_file() {
        anyhow::bail!(
            "root index.html must be a regular file: {}",
            index_html_path.display()
        );
    }

    let mut files = Vec::new();
    walk_directory(&canonical, &canonical, &mut files)?;
    files.sort_by(|a, b| a.relative.cmp(&b.relative));

    Ok(StaticSource {
        root: canonical,
        files,
    })
}

fn walk_directory(root: &Path, current: &Path, files: &mut Vec<SourceFile>) -> Result<()> {
    let read_dir = fs::read_dir(current)
        .with_context(|| format!("failed to read directory: {}", current.display()))?;

    let mut entries = Vec::new();
    for entry in read_dir {
        let entry = entry
            .with_context(|| format!("failed to read directory entry in {}", current.display()))?;
        entries.push(entry);
    }
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let file_name = entry.file_name();
        let path = entry.path();

        if current == root && file_name == ".git" {
            continue;
        }

        let ft = entry
            .file_type()
            .with_context(|| format!("failed to read file type for {}", path.display()))?;

        if ft.is_symlink() {
            anyhow::bail!(
                "symlinks are not permitted in source directory: {}",
                path.display()
            );
        }

        if ft.is_dir() {
            walk_directory(root, &path, files)?;
        } else if ft.is_file() {
            let relative = path
                .strip_prefix(root)
                .with_context(|| format!("failed to make path relative: {}", path.display()))?
                .to_path_buf();
            files.push(SourceFile { relative });
        } else {
            anyhow::bail!(
                "unsupported special file type in source directory: {}",
                path.display()
            );
        }
    }

    Ok(())
}

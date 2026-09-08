use crate::detector::StaticSource;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutput {
    pub rootfs_path: PathBuf,
    pub image_path: PathBuf,
}

pub trait ImageBuilder: Send + Sync {
    fn build(
        &self,
        root: &Path,
        deployment_id: Uuid,
        source: &StaticSource,
        base_image: &Path,
    ) -> Result<BuildOutput>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MinidockImageBuilder;

impl ImageBuilder for MinidockImageBuilder {
    fn build(
        &self,
        root: &Path,
        deployment_id: Uuid,
        source: &StaticSource,
        base_image: &Path,
    ) -> Result<BuildOutput> {
        build_deployment_image(root, deployment_id, source, base_image)
    }
}

pub fn build_deployment_image(
    state_root: &Path,
    deployment_id: Uuid,
    source: &StaticSource,
    base_image: &Path,
) -> Result<BuildOutput> {
    let build_dir = state_root.join("builds").join(deployment_id.to_string());
    let rootfs_path = build_dir.join("rootfs");
    let image_path = build_dir.join("image.tar.gz");

    let run_build = || -> Result<BuildOutput> {
        fs::create_dir_all(&rootfs_path).with_context(|| {
            format!(
                "failed to create rootfs directory at {}",
                rootfs_path.display()
            )
        })?;

        minidock::extract_rootfs(base_image, &rootfs_path).with_context(|| {
            format!(
                "failed to extract base rootfs from {}",
                base_image.display()
            )
        })?;

        let app_dir = rootfs_path.join("srv").join("app");
        fs::create_dir_all(&app_dir).with_context(|| {
            format!(
                "failed to create /srv/app directory at {}",
                app_dir.display()
            )
        })?;

        for file in &source.files {
            if !file.relative.is_relative() {
                anyhow::bail!("file path is not relative: {}", file.relative.display());
            }
            for comp in file.relative.components() {
                match comp {
                    Component::Normal(_) => {}
                    _ => {
                        anyhow::bail!("invalid path component in source file: {:?}", comp);
                    }
                }
            }

            let dest_file = app_dir.join(&file.relative);
            if !dest_file.starts_with(&app_dir) {
                anyhow::bail!(
                    "destination file {} traverses outside of {}",
                    dest_file.display(),
                    app_dir.display()
                );
            }

            if let Some(parent) = dest_file.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create directory at {}", parent.display())
                })?;
            }

            let src_file = source.root.join(&file.relative);
            fs::copy(&src_file, &dest_file).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    src_file.display(),
                    dest_file.display()
                )
            })?;
        }

        minidock::build_image(&rootfs_path, &image_path).with_context(|| {
            format!("failed to build minidock image at {}", image_path.display())
        })?;

        Ok(BuildOutput {
            rootfs_path,
            image_path,
        })
    };

    match run_build() {
        Ok(output) => Ok(output),
        Err(err) => {
            if build_dir.exists() {
                let _ = fs::remove_dir_all(&build_dir);
            }
            Err(err)
        }
    }
}

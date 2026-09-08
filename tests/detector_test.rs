use deploy_platform::detector::detect_static_source;
use std::fs;
use tempfile::tempdir;

#[test]
fn detects_root_index_and_lists_regular_files() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("index.html"), "hello").unwrap();
    fs::create_dir(dir.path().join("assets")).unwrap();
    fs::write(dir.path().join("assets/app.css"), "body{}").unwrap();
    let source = detect_static_source(dir.path()).unwrap();
    assert_eq!(
        source
            .files
            .iter()
            .map(|file| file.relative.clone())
            .collect::<Vec<_>>(),
        vec![
            std::path::PathBuf::from("assets/app.css"),
            std::path::PathBuf::from("index.html"),
        ]
    );
}

#[test]
fn rejects_a_source_without_root_index_html() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "not a site").unwrap();
    let error = detect_static_source(dir.path()).unwrap_err();
    assert!(error.to_string().contains("root index.html"));
}

#[cfg(unix)]
#[test]
fn rejects_source_symlinks_instead_of_following_them() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("index.html"), "hello").unwrap();
    std::os::unix::fs::symlink("/etc/passwd", dir.path().join("leak")).unwrap();
    let error = detect_static_source(dir.path()).unwrap_err();
    assert!(error.to_string().contains("symlink"));
}

#[test]
fn skips_the_source_git_directory() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("index.html"), "hello").unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".git/config"), "secret metadata").unwrap();
    let source = detect_static_source(dir.path()).unwrap();
    assert!(source
        .files
        .iter()
        .all(|file| !file.relative.starts_with(".git")));
}

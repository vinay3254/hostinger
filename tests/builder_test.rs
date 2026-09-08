use deploy_platform::{builder::build_deployment_image, detector::detect_static_source};
use minidock::{build_image, extract_rootfs};
use std::fs;
use tempfile::tempdir;
use uuid::Uuid;

#[test]
fn assembled_image_contains_base_files_and_static_files_under_srv_app() {
    let root = tempdir().unwrap();
    let base_context = root.path().join("base-context");
    fs::create_dir_all(base_context.join("bin")).unwrap();
    fs::write(base_context.join("bin/busybox"), "placeholder").unwrap();
    let base_image = root.path().join("base.tar.gz");
    build_image(&base_context, &base_image).unwrap();

    let source_dir = root.path().join("site");
    fs::create_dir_all(source_dir.join("assets")).unwrap();
    fs::write(source_dir.join("index.html"), "<h1>hello</h1>").unwrap();
    fs::write(source_dir.join("assets/app.css"), "body{}").unwrap();
    let source = detect_static_source(&source_dir).unwrap();
    let output = build_deployment_image(root.path(), Uuid::new_v4(), &source, &base_image).unwrap();

    let extracted = root.path().join("verify");
    extract_rootfs(&output.image_path, &extracted).unwrap();
    assert_eq!(
        fs::read_to_string(extracted.join("bin/busybox")).unwrap(),
        "placeholder"
    );
    assert_eq!(
        fs::read_to_string(extracted.join("srv/app/index.html")).unwrap(),
        "<h1>hello</h1>"
    );
    assert_eq!(
        fs::read_to_string(extracted.join("srv/app/assets/app.css")).unwrap(),
        "body{}"
    );
}

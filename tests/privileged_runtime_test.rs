use deploy_platform::{
    builder::MinidockImageBuilder,
    model::CreateProjectInput,
    runtime::MinidockRuntime,
    service::{DeploymentService, PlatformService},
    store::StateStore,
};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
};
use tempfile::tempdir;

#[test]
#[ignore = "requires root, cgroups v1, a working minidock base image, and PLATFORM_BASE_IMAGE"]
fn real_static_deployment_serves_index_html() {
    let image = std::env::var("PLATFORM_BASE_IMAGE")
        .expect("PLATFORM_BASE_IMAGE must point to a minidock rootfs tar.gz");
    assert!(std::path::Path::new(&image).is_file());
    let root = tempdir().unwrap();
    let source = root.path().join("site");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.html"), "m1 smoke test").unwrap();
    let service = DeploymentService::new(
        StateStore::at(root.path().join("platform-state")),
        MinidockRuntime {
            minidock_store: minidock::StateStore::from_current_user().unwrap(),
        },
        MinidockImageBuilder,
    );
    let project = service
        .create_project(CreateProjectInput {
            name: "smoke".into(),
            source_dir: source,
            base_image: PathBuf::from(image),
            server_command: vec![
                "/bin/busybox".into(),
                "httpd".into(),
                "-f".into(),
                "-p".into(),
                "{PORT}".into(),
                "-h".into(),
                "/srv/app".into(),
            ],
            user_id: None,
        })
        .unwrap();
    let deployment = service.deploy(project.id).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", deployment.port.unwrap())).unwrap();
    stream
        .write_all(b"GET /index.html HTTP/1.0\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.contains("m1 smoke test"));
    assert!(service.logs(deployment.id).is_ok());
    service.stop(deployment.id).unwrap();
}

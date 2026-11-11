use agent_fabric::{config::Config, invocation::Executor};
use serde_json::json;
use std::path::{Path, PathBuf};
fn config() -> Config {
    let mut config =
        Config::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fabric.example.yaml")).unwrap();
    config.controls_socket =
        PathBuf::from(std::env::var("FABRIC_TEST_SOCKET").expect("run via tests/host/main.go"));
    config
}
#[test]
#[ignore = "requires private standalone Controls; use tests/host/main.go"]
fn supervised_completion_and_failures() {
    let config = config();
    let dir = tempfile::tempdir().unwrap();
    let executor = Executor::new(&config, dir.path()).unwrap();
    let package = &config.packages[0];
    let capability = &package.capabilities[0];
    let running = executor
        .launch(
            &uuid::Uuid::new_v4().to_string(),
            package,
            capability,
            &json!({"message":"hello"}),
            None,
        )
        .unwrap();
    assert_eq!(executor.wait(&running).unwrap(), json!({"message":"hello"}));
    let mut invalid = capability.clone();
    invalid.output_schema = json!({"type":"integer"});
    let running = executor
        .launch(
            &uuid::Uuid::new_v4().to_string(),
            package,
            &invalid,
            &json!({}),
            None,
        )
        .unwrap();
    assert!(executor
        .wait(&running)
        .unwrap_err()
        .to_string()
        .contains("contract violation"));
    let mut failed = package.clone();
    failed.entrypoint = PathBuf::from("absent-program");
    assert!(executor
        .launch(
            &uuid::Uuid::new_v4().to_string(),
            &failed,
            capability,
            &json!({}),
            None
        )
        .is_err());
    let mut exited = package.clone();
    exited.path = PathBuf::from("/usr/bin");
    exited.entrypoint = PathBuf::from("true");
    let running = executor
        .launch(
            &uuid::Uuid::new_v4().to_string(),
            &exited,
            capability,
            &json!({}),
            None,
        )
        .unwrap();
    assert!(executor
        .wait(&running)
        .unwrap_err()
        .to_string()
        .contains("without an accepted result"));
}

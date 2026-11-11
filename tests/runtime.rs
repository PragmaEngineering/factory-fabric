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

fn await_terminal(
    service: &std::sync::Arc<agent_fabric::service::Service>,
    id: &str,
    caller: &str,
) -> agent_fabric::service::Invocation {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let v = service.get(id, caller).unwrap();
        if agent_fabric::service::terminal(&v.state) {
            return v;
        }
        assert!(
            std::time::Instant::now() < until,
            "invocation stalled: {} {:?}",
            v.state,
            v.error
        );
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}
#[test]
#[ignore = "requires private standalone Controls; use tests/host/main.go"]
fn concurrent_durable_invocations_and_cancellation() {
    use agent_fabric::service::{Request, Service};
    let mut config = config();
    config.concurrency = 2;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(config.clone(), dir.path()).unwrap();
    let caller = service.executor.comms.connection.participant_id.clone();
    let request = |key: &str, delay: f64| Request {
        request_key: key.into(),
        package: "utility".into(),
        capability: "echo".into(),
        input: json!({"_delay":delay}),
        working_directory: None,
    };
    let a = service.submit(&caller, request("a", 1.0)).unwrap();
    let b = service.submit(&caller, request("b", 1.0)).unwrap();
    assert_eq!(service.submit(&caller, request("a", 1.0)).unwrap().id, a.id);
    assert!(service
        .submit(&caller, request("a", 2.0))
        .err()
        .unwrap()
        .to_string()
        .contains("operation_conflict"));
    assert!(service
        .submit(&caller, request("excess", 1.0))
        .err()
        .unwrap()
        .to_string()
        .contains("capacity_exhausted"));
    assert!(service.get(&a.id, "outsider").is_err());
    let a = await_terminal(&service, &a.id, &caller);
    let b = await_terminal(&service, &b.id, &caller);
    assert_eq!(a.state, "completed", "{:?}", a.error);
    assert_eq!(b.state, "completed", "{:?}", b.error);
    let a_session = service
        .executor
        .session(a.running.as_ref().unwrap())
        .unwrap();
    let b_session = service
        .executor
        .session(b.running.as_ref().unwrap())
        .unwrap();
    assert!(a_session["started_at"].as_str().unwrap() < b_session["ended_at"].as_str().unwrap());
    assert!(b_session["started_at"].as_str().unwrap() < a_session["ended_at"].as_str().unwrap());
    let c = service.submit(&caller, request("cancel", 10.0)).unwrap();
    service.cancel(&c.id, &caller).unwrap();
    assert_eq!(await_terminal(&service, &c.id, &caller).state, "cancelled");
    let mut invalid = request("invalid", 0.0);
    invalid.capability = "inspect".into();
    invalid.input = json!({});
    assert!(service.submit(&caller, invalid).is_err());
    // Reopen after threads finish: results remain, with no duplicate dispatch.
    std::thread::sleep(std::time::Duration::from_millis(200));
    drop(service);
    let reopened = Service::open(config, dir.path()).unwrap();
    assert_eq!(
        reopened.get(&a.id, &caller).unwrap().output.unwrap()["_delay"].as_f64(),
        Some(1.0)
    );
}
#[test]
#[ignore = "requires private standalone Controls; use tests/host/main.go"]
fn service_restart_observes_existing_session() {
    use agent_fabric::controls::Controls;
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let config = config();
    let dir = tempfile::Builder::new()
        .prefix("fab-")
        .tempdir_in("/tmp")
        .unwrap();
    let config_path = dir.path().join("fabric.yaml");
    std::fs::write(&config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    let state = dir.path().join("state");
    let spawn = || {
        Command::new(env!("CARGO_BIN_EXE_fabric"))
            .arg("serve")
            .arg(&config_path)
            .arg(&state)
            .stdin(Stdio::null())
            .spawn()
            .unwrap()
    };
    let socket = state.join("fabric.sock");
    let client = Controls::new(&socket).unwrap();
    let wait_ready = || {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if client
                .call(
                    "POST",
                    "/rpc",
                    Some(&json!({"method":"catalog","params":{}})),
                )
                .is_ok()
            {
                break;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let mut host = spawn();
    wait_ready();
    let response=client.call("POST","/rpc",Some(&json!({"method":"invoke","params":{"request_key":"restart","package":"utility","capability":"echo","input":{"_delay":2}}}))).unwrap();
    let id = response["result"]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let v = client
            .call(
                "POST",
                "/rpc",
                Some(&json!({"method":"get","params":{"id":id}})),
            )
            .unwrap();
        if v["result"]["running"].is_object() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(50));
    }
    host.kill().unwrap();
    host.wait().unwrap();
    let mut host = spawn();
    wait_ready();
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let v = client
            .call(
                "POST",
                "/rpc",
                Some(&json!({"method":"get","params":{"id":id}})),
            )
            .unwrap();
        if v["result"]["state"] == "completed" {
            break;
        }
        assert!(Instant::now() < end, "{v}");
        std::thread::sleep(Duration::from_millis(50));
    }
    host.kill().unwrap();
    host.wait().unwrap();
}

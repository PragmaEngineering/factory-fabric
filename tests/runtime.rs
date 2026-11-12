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
        parent_id: None,
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
                    Some(&json!({"method":"attach","params":{"key":"restart-test","name":"restart-test"}})),
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
    let attached = client
        .call(
            "POST",
            "/rpc",
            Some(&json!({"method":"attach","params":{"key":"restart-test","name":"restart-test"}})),
        )
        .unwrap();
    let token = attached["result"]["token"].as_str().unwrap();
    let response=client.call("POST","/rpc",Some(&json!({"token":token,"method":"invoke","params":{"request_key":"restart","package":"utility","capability":"echo","input":{"_delay":2}}}))).unwrap();
    let id = response["result"]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let v = client
            .call(
                "POST",
                "/rpc",
                Some(&json!({"token":token,"method":"get","params":{"id":id}})),
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
                Some(&json!({"token":token,"method":"get","params":{"id":id}})),
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

#[test]
#[ignore = "requires private standalone Controls; use tests/host/main.go"]
fn live_registration_and_pinned_contracts() {
    use agent_fabric::{
        controls::Comms,
        registry::RegistrationRequest,
        service::{Request, Service},
    };
    use std::time::{Duration, Instant};
    let config = config();
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(config.clone(), dir.path()).unwrap();
    let controller = service.executor.comms.connection.participant_id.clone();
    let identity = service
        .executor
        .controls
        .register(&service.executor.scope, "external-test", "external")
        .unwrap();
    let external = Comms::new(identity.clone()).unwrap();
    let mut capability = config.packages[0].capabilities[0].clone();
    capability.output_schema = json!({"type":"string"});
    let mut request = RegistrationRequest {
        name: "connected".into(),
        capabilities: vec![capability],
        lease_seconds: 30,
        allow_delegation: false,
    };
    let registration = service
        .register(&identity.participant_id, request.clone())
        .unwrap();
    assert!(service
        .catalog()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["package"] == "connected" && v["availability"] == "connected"));
    let invocation = service
        .submit(
            &controller,
            Request {
                request_key: "external".into(),
                package: "connected".into(),
                capability: "echo".into(),
                input: json!({"task":"inspect"}),
                working_directory: None,
                parent_id: None,
            },
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let dispatch = loop {
        let batch = external
            .call("read_messages", json!({"thread_id":registration.mailbox}))
            .unwrap();
        if let Some(message) = batch["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["payload"]["kind"] == "fabric.invoke")
        {
            break message["payload"]["parts"][0]["data"].clone();
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(30));
    };
    request.capabilities[0].version = "2".into();
    request.capabilities[0].output_schema = json!({"type":"integer"});
    service
        .register(&identity.participant_id, request.clone())
        .unwrap();
    external.call("send_message",json!({"operation_id":"external-result","thread_id":dispatch["thread_id"],"recipients":[dispatch["result_recipient"]],"payload":{"kind":"fabric.result","version":1,"parts":[{"kind":"data","data":{"invocation_id":invocation.id,"output":"version one result"}}]}})).unwrap();
    let result = await_terminal(&service, &invocation.id, &controller);
    assert_eq!(result.state, "completed", "{:?}", result.error);
    assert_eq!(result.capability.version, "1");
    assert!(service.withdraw(&controller, "connected").is_err());
    service
        .withdraw(&identity.participant_id, "connected")
        .unwrap();
    assert!(service
        .catalog()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["package"] == "connected" && v["availability"] == "offline"));
    request.lease_seconds = 1;
    service
        .register(&identity.participant_id, request.clone())
        .unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    assert!(service
        .catalog()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["package"] == "connected" && v["availability"] == "offline"));
    request.lease_seconds = 30;
    service.register(&identity.participant_id, request).unwrap();
    service
        .executor
        .controls
        .retire(&identity.participant_id)
        .unwrap();
    assert!(service
        .catalog()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["package"] == "connected" && v["availability"] == "unknown"));
}

#[test]
#[ignore = "requires private standalone Controls and official MCP client; use tests/host/main.go"]
fn official_mcp_client_interoperability() {
    use agent_fabric::client::Client;
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let config = config();
    let dir = tempfile::Builder::new()
        .prefix("fab-mcp-")
        .tempdir_in("/tmp")
        .unwrap();
    let config_path = dir.path().join("fabric.yaml");
    std::fs::write(&config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    let state = dir.path().join("state");
    let socket = state.join("fabric.sock");
    let host = Command::new(env!("CARGO_BIN_EXE_fabric"))
        .arg("serve")
        .arg(&config_path)
        .arg(&state)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    struct Stop(std::process::Child);
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _stop = Stop(host);
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if Client::attach(&socket, "probe").is_ok() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(50));
    }
    let status = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mcp-client.mjs"))
        .arg(env!("CARGO_BIN_EXE_fabric"))
        .arg(&socket)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
#[ignore = "requires private standalone Controls; use tests/host/main.go"]
fn direct_peer_delegation_permissions_and_capacity() {
    use agent_fabric::{client::Client, service::terminal};
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for (capacity, allowed, expected) in [
        (2, true, None),
        (1, true, Some("capacity_exhausted")),
        (2, false, Some("not_authorized")),
    ] {
        let mut config = config();
        config.concurrency = capacity;
        let mut reviewer = config.packages[0].clone();
        reviewer.name = "reviewer".into();
        reviewer.entrypoint = PathBuf::from("reviewer.sh");
        reviewer.args = vec![env!("CARGO_BIN_EXE_fabric").into()];
        reviewer.allow_delegation = allowed;
        reviewer.capabilities.truncate(1);
        reviewer.capabilities[0].name = "review".into();
        config.packages.push(reviewer);
        let dir = tempfile::Builder::new()
            .prefix("fab-peer-")
            .tempdir_in("/tmp")
            .unwrap();
        let config_path = dir.path().join("fabric.yaml");
        std::fs::write(&config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
        let state = dir.path().join("state");
        let socket = state.join("fabric.sock");
        let host = Command::new(env!("CARGO_BIN_EXE_fabric"))
            .arg("serve")
            .arg(&config_path)
            .arg(&state)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        struct Stop(std::process::Child);
        impl Drop for Stop {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let _stop = Stop(host);
        let end = Instant::now() + Duration::from_secs(10);
        let client = loop {
            if let Ok(c) = Client::attach(&socket, "peer-operator") {
                break c;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(30));
        };
        let input = expected
            .map(|e| json!({"expect_error":e}))
            .unwrap_or(json!({}));
        let accepted=client.call("invoke",json!({"request_key":"review","package":"reviewer","capability":"review","input":input})).unwrap();
        let result = client
            .call("wait", json!({"id":accepted["id"],"seconds":15}))
            .unwrap();
        assert!(terminal(result["state"].as_str().unwrap()), "{result}");
        assert_eq!(result["state"], "completed", "{result}");
        if let Some(expected) = expected {
            assert_eq!(result["output"]["handled_error"], expected)
        } else {
            assert_eq!(result["output"]["specialist"]["finding"], "reviewed");
            assert!(result["output"]["peer_answer"]
                .as_str()
                .unwrap()
                .contains("Specialist"));
            let child = client
                .call("get", json!({"id":result["output"]["child_id"]}))
                .unwrap();
            assert_eq!(child["parent_id"], result["id"]);
            assert_eq!(child["caller"], result["running"]["worker_id"]);
            assert_eq!(child["root_caller"], client.connection.participant_id);
        }
    }
}

#[test]
#[ignore = "requires private standalone Controls; use tests/host/main.go"]
fn deadlines_invalid_output_and_process_failure() {
    use agent_fabric::service::{Request, Service};
    let mut config = config();
    config.packages[0].elapsed_seconds = 1;
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(config, dir.path()).unwrap();
    let caller = service.executor.comms.connection.participant_id.clone();
    let request = |key: &str, input| Request {
        request_key: key.into(),
        package: "utility".into(),
        capability: "echo".into(),
        input,
        working_directory: None,
        parent_id: None,
    };
    let timeout = service
        .submit(&caller, request("timeout", json!({"_delay":10})))
        .unwrap();
    let timeout = await_terminal(&service, &timeout.id, &caller);
    assert_eq!(timeout.state, "failed");
    assert!(timeout.error.unwrap().contains("elapsed time"));
    assert_ne!(
        service
            .executor
            .session(timeout.running.as_ref().unwrap())
            .unwrap()["state"],
        "running"
    );
    let crash = service
        .submit(&caller, request("crash", json!({"_crash":true})))
        .unwrap();
    let crash = await_terminal(&service, &crash.id, &caller);
    assert_eq!(crash.state, "failed");
    assert!(crash.error.unwrap().contains("without accepted result"));
}
#[test]
#[ignore = "requires private standalone Controls; use tests/host/main.go"]
fn comms_listener_reconnect_retains_results() {
    use agent_fabric::{
        controls::{Comms, Controls},
        invocation::Executor,
    };
    let config = config();
    let dir = tempfile::tempdir().unwrap();
    let mut executor = Executor::new(&config, dir.path()).unwrap();
    // Simulate the stale endpoint left by a listener reconnect/rebind. The
    // private host API remains the authoritative source of the current URL.
    let mut connection = executor.comms.connection.clone();
    let current = Comms::new(connection.clone()).unwrap();
    let thread = current
        .call(
            "create_thread",
            json!({"operation_id":"reconnect-thread","title":"Reconnect","participants":[]}),
        )
        .unwrap();
    connection.url = "http://127.0.0.1:1/mcp".into();
    executor.comms.connection.url = connection.url;
    let batch = executor
        .comms_call("read_messages", json!({"thread_id":thread["thread_id"]}))
        .unwrap();
    assert!(batch["messages"].as_array().unwrap().is_empty());
    let controls = Controls::new(&config.controls_socket).unwrap();
    let scope = controls.scope("another-fabric").unwrap();
    let foreign = controls.register(&scope, "foreign", "foreign").unwrap();
    assert!(current.call("create_thread",json!({"operation_id":"cross-scope","title":"No access","participants":[foreign.participant_id]})).is_err());
}

#[test]
#[ignore = "requires standalone Controls and sibling Registry; use tests/host/main.go"]
fn readiness_uses_assignment_mcp_without_context_files() {
    let mut config = config();
    let controls = PathBuf::from(std::env::var("FABRIC_TEST_CONTROLS").unwrap());
    let registry = controls.parent().unwrap().join("registry");
    let response = std::fs::read(registry.join("tests/fixtures/readiness-response.json")).unwrap();
    let provider = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", provider.server_addr());
    let provider_thread = std::thread::spawn(move || {
        let request = provider
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap()
            .unwrap();
        assert!(request
            .headers()
            .iter()
            .any(|h| h.field.equiv("Authorization")
                && h.value.as_str() == "Bearer fixture-provider-key"));
        request
            .respond(tiny_http::Response::from_data(response).with_header(
                tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
            ))
            .unwrap();
    });
    let package = &mut config.packages[0];
    package.name = "readiness".into();
    package.protocol = "mcp".into();
    package.path = registry.join("classifiers/task-readiness");
    package.entrypoint = "start.sh".into();
    package.args.clear();
    package.environment.insert(
        "OPENROUTER_API_KEY".into(),
        "FABRIC_TEST_PROVIDER_SECRET".into(),
    );
    package.overrides.insert("endpoint".into(), json!(endpoint));
    package.capabilities.truncate(1);
    package.capabilities[0].name = "assess".into();
    let root = tempfile::tempdir().unwrap();
    let executor = Executor::new(&config, root.path()).unwrap();
    let running = executor
        .launch(
            &uuid::Uuid::new_v4().to_string(),
            &config.packages[0],
            &config.packages[0].capabilities[0],
            &json!({"task":"Investigate gateway latency","context":{"target":"gateway"}}),
            None,
        )
        .unwrap();
    let output = executor.wait(&running).unwrap();
    assert_eq!(output["outcome"], "completed");
    assert_eq!(
        output["data"]["response"]["answers"]["readiness"]["choice"],
        "clarify"
    );
    assert!(!root.path().join(&running.id).join("context.json").exists());
    assert!(!root.path().join(&running.id).join("input.json").exists());
    provider_thread.join().unwrap();
}

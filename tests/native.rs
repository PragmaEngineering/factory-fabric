//! Optional authenticated harness smoke test. Uses the user's configured model
//! and credentials, with command-line-only MCP configuration in a temporary cwd.
use agent_fabric::{client::Client, config::Config};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
struct Stop(std::process::Child);
impl Drop for Stop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
#[ignore = "requires authenticated Codex; FABRIC_TEST_SUITE=native go run tests/host/main.go"]
fn native_codex_uses_fabric_mcp() {
    let mut config =
        Config::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fabric.example.yaml")).unwrap();
    config.controls_socket = PathBuf::from(std::env::var("FABRIC_TEST_SOCKET").unwrap());
    let dir = tempfile::Builder::new()
        .prefix("fab-native-")
        .tempdir_in("/tmp")
        .unwrap();
    let config_path = dir.path().join("fabric.yaml");
    fs::write(&config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    let state = dir.path().join("state");
    let socket = state.join("fabric.sock");
    let binary = env!("CARGO_BIN_EXE_fabric");
    let _host = Stop(
        Command::new(binary)
            .arg("serve")
            .arg(&config_path)
            .arg(&state)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + Duration::from_secs(10);
    let client = loop {
        if let Ok(c) = Client::attach(&socket, "native-smoke") {
            break c;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(50));
    };
    let workspace = dir.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let result_file = dir.path().join("result.json");
    let schema = dir.path().join("result-schema.json");
    fs::write(&schema,json!({"type":"object","properties":{"invocation_id":{"type":"string"},"state":{"type":"string"},"output":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false}},"required":["invocation_id","state","output"],"additionalProperties":false}).to_string()).unwrap();
    let events = dir.path().join("events.jsonl");
    let diagnostics = dir.path().join("diagnostics.txt");
    let prompt=format!("Run this bounded integration smoke test using ONLY the fabric MCP tools. Do not run shell commands, modify files, use other services, or delegate outside Fabric. Discover available capabilities with agents_list, inspect utility.echo with agents_describe, invoke utility.echo with request_key native-smoke-1 and input {{\"message\":\"native-codex-smoke\"}}, and retrieve its completed result. Return the invocation ID, terminal state, and actual output as the required JSON. Follow the supplied Fabric skill.\n\n{}",include_str!("../skills/fabric/SKILL.md"));
    let mut command = Command::new("codex");
    command
        .arg("exec")
        .args([
            "--ephemeral",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--json",
            "-C",
        ])
        .arg(&workspace)
        .arg("--output-last-message")
        .arg(&result_file)
        .arg("--output-schema")
        .arg(&schema)
        .arg("-c")
        .arg(format!("mcp_servers.fabric.command={}", json!(binary)))
        .arg("-c")
        .arg(format!(
            "mcp_servers.fabric.args={}",
            json!(["mcp", socket, "--identity", "native-smoke"])
        ))
        .args([
            "-c",
            "mcp_servers.fabric.required=true",
            "-c",
            "mcp_servers.fabric.default_tools_approval_mode=\"approve\"",
            "-c",
            "mcp_servers.node_repl.enabled=false",
        ])
        .arg(prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::from(fs::File::create(&events).unwrap()))
        .stderr(Stdio::from(fs::File::create(&diagnostics).unwrap()));
    let mut native = Stop(command.spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if let Some(status) = native.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < until,
            "native Codex exceeded smoke-test deadline"
        );
        std::thread::sleep(Duration::from_millis(200));
    };
    assert!(
        status.success(),
        "native Codex failed: {}",
        fs::read_to_string(&diagnostics)
            .unwrap()
            .chars()
            .rev()
            .take(2000)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    );
    let response: Value = serde_json::from_slice(&fs::read(&result_file).unwrap()).unwrap();
    assert_eq!(response["state"], "completed");
    assert_eq!(response["output"]["message"], "native-codex-smoke");
    let actual = client
        .call("get", json!({"id":response["invocation_id"]}))
        .unwrap();
    assert_eq!(actual["state"], "completed");
    assert_eq!(actual["output"], response["output"]);
    let events = fs::read_to_string(events).unwrap();
    assert!(events.contains("agents_invoke"));
    assert!(events.contains("agents_list"));
    println!("Native Codex smoke: discovered capabilities, invoked utility.echo through MCP, and returned the verified durable result.");
}

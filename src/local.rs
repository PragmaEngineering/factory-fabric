//! Foreground ownership of the local stack; Controls still owns DB and sessions.
use crate::{config::Config, service};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use std::{
    fs::{self, OpenOptions},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

struct Host {
    child: Child,
    http: reqwest::blocking::Client,
    endpoint: String,
    token: String,
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self
            .http
            .post(format!("{}/shutdown", self.endpoint))
            .bearer_auth(&self.token)
            .send();
        // The host may still be starting and not yet accept /shutdown.
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &self.child.id().to_string()])
            .stderr(Stdio::null())
            .status();
        let deadline = Instant::now() + Duration::from_secs(25);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        eprintln!("Controls has not stopped yet; inspect controls.log before restarting.");
    }
}

pub fn up(path: &Path) -> Result<()> {
    let path = path
        .canonicalize()
        .context("configuration missing; try fabric up fabric.example.yaml")?;
    let mut config = Config::load(&path)?;
    let root = path.parent().unwrap().join(".fabric");
    if config.controls_endpoint.is_none()
        && config.controls_socket != root.join("controls/factory.sock")
    {
        bail!("fabric up manages .fabric/controls/factory.sock; omit controls_socket from configuration. Use serve for an external Controls host")
    }
    fs::create_dir_all(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let lease = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("up.lock"))?;
    lease
        .try_lock_exclusive()
        .context("this project's local stack is already running")?;
    let fabric_token_file = root.join("api-token");
    if !fabric_token_file.exists() {
        crate::invocation::private_file(
            &fabric_token_file,
            format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()).as_bytes(),
        )?;
    }
    std::env::set_var("FABRIC_API_TOKEN_FILE", &fabric_token_file);
    if std::env::var_os("FABRIC_LISTEN").is_none() {
        std::env::set_var("FABRIC_LISTEN", "127.0.0.1:0");
    }
    if config.controls_endpoint.is_some() {
        if config.fabric_endpoint.is_none() {
            bail!("remote Controls requires fabric_endpoint reachable from the execution host")
        }
        return service::serve(config, &root);
    }
    let controls_token_file = root.join("controls-token");
    if !controls_token_file.exists() {
        crate::invocation::private_file(
            &controls_token_file,
            format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()).as_bytes(),
        )?;
    }
    let controls_token = fs::read_to_string(&controls_token_file)?;
    std::env::set_var("CONTROLS_TOKEN", controls_token.trim());
    let executable = std::env::current_exe()?;
    let bin = executable.parent().unwrap();
    let host_binary = bin.join("factory-runtime");
    if !host_binary.is_file() {
        bail!(
            "missing {}; run ./scripts/build.sh once from the Fabric checkout",
            host_binary.display()
        )
    }
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("controls.log"))?;
    fs::set_permissions(root.join("controls.log"), fs::Permissions::from_mode(0o600))?;
    let mut paths = vec![bin.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let child = Command::new(host_binary)
        .args(["--root"])
        .arg(root.join("controls"))
        .arg("--local-database")
        .args(["--listen", "127.0.0.1:0", "--token-file"])
        .arg(&controls_token_file)
        .env("PATH", std::env::join_paths(paths)?)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(1))
        .build()?;
    let mut host = Host {
        child,
        http,
        endpoint: String::new(),
        token: controls_token.trim().into(),
    };
    eprintln!("Starting local Controls and PostgreSQL (first start may download PostgreSQL)…");
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if stopped.load(Ordering::Relaxed) {
            return Ok(());
        }
        if let Some(status) = host.child.try_wait()? {
            bail!(
                "Controls exited ({status}); see {}",
                root.join("controls.log").display()
            )
        }
        if let Ok(bytes) = fs::read(root.join("controls/endpoint.json")) {
            if let Ok(record) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if record["pid"].as_u64() != Some(host.child.id() as u64) {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                if let Some(endpoint) = record["url"].as_str() {
                    host.endpoint = endpoint.into();
                    if host
                        .http
                        .get(format!("{endpoint}/health"))
                        .bearer_auth(&host.token)
                        .send()
                        .is_ok_and(|r| r.status().is_success())
                    {
                        break;
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            bail!(
                "Controls startup timed out; see {}",
                root.join("controls.log").display()
            )
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!(
        "Local stack ready. Ctrl-C stops Fabric, Controls, and active agents. State: {}",
        root.display()
    );
    config.controls_endpoint = Some(host.endpoint.clone());
    config.controls_identity = Some(format!("local:{}", root.display()));
    service::serve_until(config, &root, stopped)
}

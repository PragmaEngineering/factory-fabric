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
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.http.post("http://localhost/shutdown").send();
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
    let config = Config::load(&path)?;
    let root = path.parent().unwrap().join(".fabric");
    if config.controls_socket != root.join("controls/factory.sock") {
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
        .env("PATH", std::env::join_paths(paths)?)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    let http = reqwest::blocking::Client::builder()
        .unix_socket(config.controls_socket.as_path())
        .no_proxy()
        .timeout(Duration::from_secs(1))
        .build()?;
    let mut host = Host { child, http };
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
        if host
            .http
            .get("http://localhost/health")
            .send()
            .is_ok_and(|r| r.status().is_success())
        {
            break;
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
    service::serve_until(config, &root, stopped)
}

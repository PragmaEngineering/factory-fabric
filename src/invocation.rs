use crate::{
    config::{validate_value, Capability, Config, Package},
    controls::{Comms, Controls},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn private_file(path: &Path, content: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(content)?;
    f.sync_all()?;
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Running {
    pub id: String,
    pub session_id: String,
    pub thread_id: String,
    pub worker_id: String,
    pub capability: Capability,
    pub package: Package,
}
pub struct Executor {
    pub controls: Controls,
    pub comms: Comms,
    pub scope: String,
    pub root: PathBuf,
}
impl Executor {
    pub fn new(config: &Config, root: &Path) -> Result<Self> {
        private_dir(root)?;
        let root = fs::canonicalize(root)?;
        let controls = Controls::new(&config.controls_socket)?;
        let scope = controls.scope(&format!("fabric:{}", config.name))?;
        let connection = controls.register(&scope, "fabric-controller", "fabric")?;
        let comms = Comms::new(connection)?;
        Ok(Self {
            controls,
            comms,
            scope,
            root,
        })
    }
    pub fn launch(
        &self,
        id: &str,
        package: &Package,
        capability: &Capability,
        input: &Value,
        caller: Option<&str>,
    ) -> Result<Running> {
        validate_value(&capability.input_schema, input)?;
        let worker = self.controls.register(&self.scope, id, &package.name)?;
        let mut participants = vec![worker.participant_id.clone()];
        if let Some(caller) = caller {
            participants.push(caller.to_string());
        }
        let thread = self.comms.call("create_thread", json!({"operation_id":format!("thread:{id}"),"title":format!("{}.{} {id}",package.name,capability.name),"participants":participants}))?;
        let thread_id = thread["thread_id"]
            .as_str()
            .context("missing thread id")?
            .to_string();
        let dir = self.root.join(id);
        private_dir(&dir)?;
        private_file(&dir.join("input.json"), &serde_json::to_vec(input)?)?;
        private_file(
            &dir.join("config.json"),
            &serde_json::to_vec(&package.overrides)?,
        )?;
        let prompt = format!("Execute capability {}.{} version {} with input at {}. Read FABRIC_CONTEXT_PATH for the communication connection and output schema. Report completion through send_message with payload kind fabric.result, version 1, one data part containing invocation_id and output, addressed to the result_recipient. Use a new operation_id for new content. Process exit alone is not completion.", package.name, capability.name, capability.version, dir.join("input.json").display());
        private_file(&dir.join("prompt.txt"), prompt.as_bytes())?;
        let context = json!({"invocation_id":id,"capability":capability,"connection":worker,"thread_id":thread_id,"result_recipient":self.comms.connection.participant_id});
        private_file(&dir.join("context.json"), &serde_json::to_vec(&context)?)?;
        let mut environment = BTreeMap::new();
        for (target, source) in &package.environment {
            environment.insert(
                target.clone(),
                std::env::var(source)
                    .with_context(|| format!("missing environment reference {source}"))?,
            );
        }
        for (key, value) in [
            ("FABRIC_CONTEXT_PATH", dir.join("context.json")),
            ("FACTORY_INPUTS_PATH", dir.join("input.json")),
            ("FACTORY_PACKAGE_CONFIG", dir.join("config.json")),
            ("FACTORY_PROMPT_PATH", dir.join("prompt.txt")),
            ("FACTORY_INVOCATION_DIR", dir.clone()),
        ] {
            environment.insert(key.into(), value.to_string_lossy().into_owned());
        }
        let mut command = vec![package
            .path
            .join(&package.entrypoint)
            .to_string_lossy()
            .into_owned()];
        command.extend(package.args.iter().map(|s| s.replace("{prompt}", &prompt)));
        let session = self.controls.call("POST", "/sessions", Some(&json!({"name":format!("fabric:{id}"),"runner":"command","input_mode":"none","directory":package.working_directory,"command":command,"environment":environment})))?;
        Ok(Running {
            id: id.to_string(),
            session_id: session["id"]
                .as_str()
                .context("missing session id")?
                .to_string(),
            thread_id,
            worker_id: worker.participant_id,
            capability: capability.clone(),
            package: package.clone(),
        })
    }
    pub fn poll_result(&self, running: &Running, cursor: &mut String) -> Result<Option<Value>> {
        let batch = self.comms.call(
            "read_messages",
            json!({"thread_id":running.thread_id,"after":cursor,"limit":200}),
        )?;
        for message in batch["messages"].as_array().context("missing messages")? {
            if message["sender"] != running.worker_id
                || message["payload"]["kind"] != "fabric.result"
            {
                continue;
            }
            let data = message["payload"]["parts"]
                .as_array()
                .and_then(|a| a.iter().find_map(|p| p.get("data")))
                .context("missing result data")?;
            if data["invocation_id"] != running.id {
                continue;
            }
            if let Some(error) = data.get("error") {
                bail!("implementation failure: {error}");
            }
            let output = data.get("output").context("result requires output")?;
            validate_value(&running.capability.output_schema, output)?;
            *cursor = batch["cursor"]
                .as_str()
                .context("missing cursor")?
                .to_string();
            return Ok(Some(output.clone()));
        }
        *cursor = batch["cursor"]
            .as_str()
            .context("missing cursor")?
            .to_string();
        Ok(None)
    }
    pub fn session(&self, running: &Running) -> Result<Value> {
        self.controls
            .call("GET", &format!("/sessions/{}", running.session_id), None)
    }
    pub fn stop(&self, running: &Running) -> Result<()> {
        let state = self.session(running)?;
        if state["state"] == "running" {
            self.controls.call(
                "POST",
                &format!("/sessions/{}/stop", running.session_id),
                Some(&json!({})),
            )?;
        }
        Ok(())
    }
    pub fn wait(&self, running: &Running) -> Result<Value> {
        let started = Instant::now();
        let mut cursor = String::new();
        let result = (|| -> Result<Value> {
            loop {
                if let Some(output) = self.poll_result(running, &mut cursor)? {
                    break Ok(output);
                }
                let session = self.session(running)?;
                if session["state"] != "running" && session["state"] != "stopping" {
                    if let Some(output) = self.poll_result(running, &mut cursor)? {
                        break Ok(output);
                    }
                    break Err(anyhow::anyhow!(
                        "session {} without an accepted result; exit_code={}",
                        session["state"],
                        session["exit_code"]
                    ));
                }
                if started.elapsed().as_secs() >= running.package.elapsed_seconds {
                    break Err(anyhow::anyhow!("invocation timed out"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        })();
        let _ = self.stop(running);
        let _ = self.controls.retire(&running.worker_id);
        result
    }
}

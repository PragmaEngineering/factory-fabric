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
pub fn private_file_or_replace(path: &Path, content: &[u8]) -> Result<()> {
    let temporary = path.with_extension("tmp");
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    private_file(&temporary, content)?;
    fs::rename(temporary, path)?;
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
#[derive(Debug)]
pub struct LaunchUncertain(pub String);
impl std::fmt::Display for LaunchUncertain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for LaunchUncertain {}
pub struct Executor {
    pub controls: Controls,
    pub comms: Comms,
    pub scope: String,
    pub root: PathBuf,
    pub network: bool,
    pub fabric_endpoint: Option<String>,
}
impl Executor {
    pub fn new(config: &Config, root: &Path) -> Result<Self> {
        private_dir(root)?;
        let root = fs::canonicalize(root)?;
        let controls = if let Some(endpoint) = &config.controls_endpoint {
            let token = std::env::var(
                config
                    .controls_token_env
                    .as_deref()
                    .unwrap_or("CONTROLS_TOKEN"),
            )
            .context("missing Controls host credential")?;
            Controls::connect(endpoint, &token)?
        } else {
            Controls::new(&config.controls_socket)?
        };
        let scope = controls.scope(&format!("fabric:{}", config.name))?;
        let connection = controls.register(&scope, "fabric-controller", "fabric")?;
        let comms = Comms::new(connection)?;
        Ok(Self {
            controls,
            comms,
            scope,
            root,
            network: config.controls_endpoint.is_some(),
            fabric_endpoint: config.fabric_endpoint.clone(),
        })
    }
    pub fn comms_call(&self, name: &str, args: Value) -> Result<Value> {
        match self.comms.call(name, args.clone()) {
            Err(e) if e.is::<reqwest::Error>() => {
                let endpoint = self.controls.call("GET", "/comms", None)?;
                let mut connection = self.comms.connection.clone();
                connection.url = endpoint["url"]
                    .as_str()
                    .context("missing comms endpoint")?
                    .into();
                Comms::new(connection)?.call(name, args)
            }
            result => result,
        }
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
        let thread = self.comms_call("create_thread", json!({"operation_id":format!("thread:{id}"),"title":format!("{id} {}.{}",package.name,capability.name).chars().take(200).collect::<String>(),"participants":participants}))?;
        let thread_id = thread["thread_id"]
            .as_str()
            .context("missing thread id")?
            .to_string();
        if package.protocol == "mcp" {
            let instructions = format!("Execute capability {}.{} version {}. Fetch your assignment with assignment_get; report output or failure with assignment_complete. Use existing communication tools for peer messages.",package.name,capability.name,capability.version);
            let assignment = json!({"version":1,"id":id,"participant_id":worker.participant_id,"thread_id":thread_id,"recipient":self.comms.connection.participant_id,"mode":"fabric","capability":capability,"input":input,"configuration":package.overrides,"instructions":instructions,"working_directory":package.working_directory,"files_directory":package.working_directory,"output_schema":capability.output_schema,"metadata":{"allow_delegation":package.allow_delegation,"fabric_endpoint":self.fabric_endpoint.clone().or_else(||fs::read(self.root.parent().unwrap().join("endpoint.json")).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok()).and_then(|v|v["url"].as_str().map(str::to_owned)))}});
            self.controls
                .call("POST", "/comms/assignments", Some(&assignment))?;
            let environment = json!({"AGENT_MCP_URL":worker.url,"AGENT_MCP_TOKEN":worker.token});
            let mut command = vec![package
                .path
                .join(&package.entrypoint)
                .to_string_lossy()
                .into_owned()];
            command.extend(
                package
                    .args
                    .iter()
                    .map(|s| s.replace("{prompt}", &instructions)),
            );
            let session = self.controls.call("POST","/launch",Some(&json!({"session":{"name":format!("fabric:{id}"),"runner":"command","input_mode":"none","directory":package.working_directory,"command":command,"environment":environment},"environment_references":package.environment,"runtime":package.runtime,"package_path":package.path,"configuration":package.overrides,"assignment_id":id}))).map_err(|e|if e.is::<reqwest::Error>() {anyhow::Error::new(LaunchUncertain(format!("{e:#}")))} else {e})?;
            return Ok(Running {
                id: id.into(),
                session_id: session["id"]
                    .as_str()
                    .ok_or_else(|| LaunchUncertain("host returned no session id".into()))?
                    .into(),
                thread_id,
                worker_id: worker.participant_id,
                capability: capability.clone(),
                package: package.clone(),
            });
        }
        let dir = if self.network {
            let prepared = self.controls.call(
                "POST",
                &format!("/workspaces/{id}"),
                Some(&json!({"files":{}})),
            )?;
            PathBuf::from(
                prepared["directory"]
                    .as_str()
                    .context("missing execution-host workspace")?,
            )
        } else {
            self.root.join(id)
        };
        let skill_path = dir.join("skills/fabric/SKILL.md");
        let prompt = format!("Read the Fabric skill at {}. Execute capability {}.{} version {} with input at {}. Read FABRIC_CONTEXT_PATH for the communication connection and output schema. Report completion through send_message with payload kind fabric.result, version 1, one data part containing invocation_id and output, addressed to the result_recipient. Process exit alone is not completion.",skill_path.display(),package.name,capability.name,capability.version,dir.join("input.json").display());
        let endpoint = if self.network {
            self.fabric_endpoint
                .clone()
                .or_else(|| {
                    fs::read(self.root.parent().unwrap().join("endpoint.json"))
                        .ok()
                        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                        .and_then(|v| v["url"].as_str().map(str::to_owned))
                })
                .context("network execution needs a reachable fabric_endpoint")?
        } else {
            self.root
                .parent()
                .unwrap()
                .join("fabric.sock")
                .to_string_lossy()
                .into_owned()
        };
        let context = json!({"invocation_id":id,"capability":capability,"connection":worker,"thread_id":thread_id,"result_recipient":self.comms.connection.participant_id,"fabric_endpoint":endpoint,"fabric_socket":endpoint,"skill_path":skill_path});
        let files = BTreeMap::from([
            ("input.json", serde_json::to_string(input)?),
            ("config.json", serde_json::to_string(&package.overrides)?),
            ("context.json", serde_json::to_string(&context)?),
            ("prompt.txt", prompt.clone()),
            (
                "skills/fabric/SKILL.md",
                include_str!("../skills/fabric/SKILL.md").into(),
            ),
        ]);
        if self.network {
            self.controls.call(
                "POST",
                &format!("/workspaces/{id}"),
                Some(&json!({"files":files})),
            )?;
        } else {
            private_dir(&dir)?;
            private_dir(&dir.join("skills/fabric"))?;
            for (name, content) in files {
                private_file(&dir.join(name), content.as_bytes())?;
            }
        }
        let mut environment = BTreeMap::new();
        if !self.network {
            for (target, source) in &package.environment {
                environment.insert(
                    target.clone(),
                    std::env::var(source)
                        .with_context(|| format!("missing environment reference {source}"))?,
                );
            }
        }
        for (key, value) in [
            ("FABRIC_CONTEXT_PATH", dir.join("context.json")),
            ("FABRIC_SKILL_PATH", skill_path),
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
        let session_request = json!({"name":format!("fabric:{id}"),"runner":"command","input_mode":"none","directory":package.working_directory,"command":command,"environment":environment});
        let (route, body) = if self.network {
            (
                "/launch",
                json!({"session":session_request,"environment_references":package.environment}),
            )
        } else {
            ("/sessions", session_request)
        };
        let session = self
            .controls
            .call("POST", route, Some(&body))
            .map_err(|e| {
                if e.is::<reqwest::Error>() {
                    anyhow::Error::new(LaunchUncertain(format!("{e:#}")))
                } else {
                    e
                }
            })?;
        Ok(Running {
            id: id.to_string(),
            session_id: session["id"]
                .as_str()
                .ok_or_else(|| LaunchUncertain("host returned no session id".into()))?
                .to_string(),
            thread_id,
            worker_id: worker.participant_id,
            capability: capability.clone(),
            package: package.clone(),
        })
    }
    pub fn poll_result(&self, running: &Running, cursor: &mut String) -> Result<Option<Value>> {
        let batch = self.comms_call(
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

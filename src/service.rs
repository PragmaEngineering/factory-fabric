use crate::{
    config::{validate_value, Capability, Config, Package},
    invocation::{private_dir, Executor, LaunchUncertain, Running},
};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
pub fn terminal(state: &str) -> bool {
    matches!(state, "completed" | "failed" | "cancelled")
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub request_key: String,
    pub package: String,
    pub capability: String,
    pub input: Value,
    #[serde(default)]
    pub working_directory: Option<std::path::PathBuf>,
    #[serde(default)]
    pub parent_id: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Invocation {
    pub id: String,
    pub caller: String,
    pub request: Request,
    pub package: Package,
    pub capability: Capability,
    pub state: String,
    pub accepted_at: u64,
    pub running: Option<Running>,
    pub cursor: String,
    pub output: Option<Value>,
    pub error: Option<String>,
    #[serde(default)]
    pub observation_error: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub root_caller: String,
    #[serde(default)]
    pub assigned_peer: Option<String>,
    #[serde(default)]
    pub external: Option<crate::registry::Registration>,
}
pub struct Service {
    pub config: Config,
    pub executor: Executor,
    pub(crate) db: Mutex<Connection>,
    _lease: fs::File,
}
impl Service {
    pub fn open(config: Config, root: &Path) -> Result<Arc<Self>> {
        private_dir(root)?;
        let lease = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(root.join("service.lock"))?;
        lease
            .try_lock_exclusive()
            .context("Fabric state directory already in use")?;
        let db = Connection::open(root.join("fabric.db"))?;
        fs::set_permissions(root.join("fabric.db"), fs::Permissions::from_mode(0o600))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS invocations(id TEXT PRIMARY KEY,caller TEXT NOT NULL,request_key TEXT NOT NULL,active INTEGER NOT NULL,body TEXT NOT NULL,UNIQUE(caller,request_key));")?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);",
        )?;
        let identity = serde_json::to_string(
            &json!({"name":config.name,"controls_socket":config.controls_socket}),
        )?;
        let recorded: Option<String> = db
            .query_row("SELECT value FROM metadata WHERE key='identity'", [], |r| {
                r.get(0)
            })
            .optional()?;
        if recorded.as_ref().is_some_and(|old| old != &identity) {
            bail!("state directory belongs to another Fabric name or Controls host")
        }
        db.execute(
            "INSERT OR IGNORE INTO metadata(key,value) VALUES('identity',?1)",
            [identity],
        )?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS registrations(name TEXT PRIMARY KEY,body TEXT NOT NULL);",
        )?;
        let executor = Executor::new(&config, &root.join("invocations"))?;
        let service = Arc::new(Self {
            config,
            executor,
            db: Mutex::new(db),
            _lease: lease,
        });
        Ok(service)
    }
    fn load(db: &Connection, id: &str) -> Result<Invocation> {
        let raw: String = db
            .query_row("SELECT body FROM invocations WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .context("unknown invocation")?;
        Ok(serde_json::from_str(&raw)?)
    }
    fn save(db: &Connection, v: &Invocation) -> Result<()> {
        db.execute("INSERT INTO invocations(id,caller,request_key,active,body) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET active=excluded.active,body=excluded.body",params![v.id,v.caller,v.request.request_key,!terminal(&v.state),serde_json::to_string(v)?])?;
        Ok(())
    }
    pub(crate) fn update(
        &self,
        id: &str,
        change: impl FnOnce(&mut Invocation),
    ) -> Result<Invocation> {
        let db = self.db.lock().unwrap();
        let mut v = Self::load(&db, id)?;
        change(&mut v);
        Self::save(&db, &v)?;
        Ok(v)
    }
    pub fn get(&self, id: &str, caller: &str) -> Result<Invocation> {
        let db = self.db.lock().unwrap();
        let v = Self::load(&db, id)?;
        if v.caller != caller
            && v.root_caller != caller
            && v.running.as_ref().is_none_or(|r| r.worker_id != caller)
        {
            bail!("not_authorized: invocation belongs to another caller")
        };
        Ok(v)
    }
    fn delegation_context(
        &self,
        db: &Connection,
        caller: &str,
        explicit_parent: Option<&str>,
    ) -> Result<(Option<String>, String)> {
        let managed:Option<String>=db.query_row("SELECT body FROM invocations WHERE json_extract(body,'$.assigned_peer')=?1 ORDER BY rowid DESC LIMIT 1",[caller],|r|r.get(0)).optional()?;
        if let Some(raw) = managed {
            let parent: Invocation = serde_json::from_str(&raw)?;
            if !parent.package.allow_delegation
                || !matches!(parent.state.as_str(), "launching" | "running")
            {
                bail!("not_authorized: this invocation cannot delegate")
            }
            if explicit_parent.is_some_and(|id| id != parent.id) {
                bail!("not_authorized: parent must be the caller's own invocation")
            }
            let root = if parent.root_caller.is_empty() {
                parent.caller.clone()
            } else {
                parent.root_caller.clone()
            };
            return Ok((Some(parent.id), root));
        }
        let operator: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM metadata WHERE key=?1 AND value='operator')",
            [format!("operator:{caller}")],
            |r| r.get(0),
        )?;
        if operator || caller == self.executor.comms.connection.participant_id {
            if let Some(id) = explicit_parent {
                let parent = Self::load(db, id)?;
                if parent.caller != caller && parent.root_caller != caller {
                    bail!("not_authorized: parent belongs to another caller")
                }
                return Ok((Some(id.into()), caller.into()));
            }
            return Ok((None, caller.into()));
        }
        let Some(parent_id) = explicit_parent else {
            bail!("not_authorized: connected workers must specify their parent invocation")
        };
        let parent = Self::load(db, parent_id)?;
        if parent
            .running
            .as_ref()
            .is_none_or(|r| r.worker_id != caller)
            || !parent.package.allow_delegation
            || parent.state != "running"
        {
            bail!("not_authorized: parent invocation does not permit this delegation")
        }
        let root = if parent.root_caller.is_empty() {
            parent.caller.clone()
        } else {
            parent.root_caller.clone()
        };
        Ok((Some(parent.id), root))
    }
    pub fn submit(self: &Arc<Self>, caller: &str, request: Request) -> Result<Invocation> {
        if request.request_key.is_empty() || request.request_key.len() > 128 {
            bail!("request_key must contain 1..128 characters")
        }
        let db = self.db.lock().unwrap();
        let prior: Option<String> = db
            .query_row(
                "SELECT body FROM invocations WHERE caller=?1 AND request_key=?2",
                params![caller, request.request_key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(raw) = prior {
            let old: Invocation = serde_json::from_str(&raw)?;
            if serde_json::to_value(&old.request)? != serde_json::to_value(&request)? {
                bail!("operation_conflict: request_key reused with different arguments")
            };
            return Ok(old);
        }
        let (parent_id, root_caller) =
            self.delegation_context(&db, caller, request.parent_id.as_deref())?;
        let (mut package, external) = if let Some(p) = self
            .config
            .packages
            .iter()
            .find(|p| p.name == request.package)
        {
            (p.clone(), None)
        } else {
            let (p, r) = self.registered_package(&db, &request.package)?;
            (p, Some(r))
        };
        let capability = package
            .capabilities
            .iter()
            .find(|c| c.name == request.capability)
            .context("unknown capability")?
            .clone();
        validate_value(&capability.input_schema, &request.input)?;
        if external.is_some() && request.working_directory.is_some() {
            bail!("external capabilities own their working environment")
        }
        if let Some(path) = &request.working_directory {
            let path = fs::canonicalize(path)?;
            if !path.is_dir() {
                bail!("working_directory must be a directory")
            };
            package.working_directory = Some(path)
        }
        let count: usize =
            db.query_row("SELECT count(*) FROM invocations WHERE active=1", [], |r| {
                r.get(0)
            })?;
        if count >= self.config.concurrency {
            bail!("capacity_exhausted: no worker slot; submission was not accepted")
        }
        let v = Invocation {
            id: uuid::Uuid::new_v4().to_string(),
            caller: caller.into(),
            request,
            package,
            capability,
            state: "accepted".into(),
            accepted_at: now(),
            running: None,
            cursor: String::new(),
            output: None,
            error: None,
            observation_error: None,
            parent_id,
            root_caller,
            assigned_peer: None,
            external,
        };
        Self::save(&db, &v)?;
        drop(db);
        self.spawn(v.id.clone());
        Ok(v)
    }
    fn spawn(self: &Arc<Self>, id: String) {
        let service = Arc::clone(self);
        std::thread::spawn(move || {
            if let Err(e) = service.drive(&id) {
                let _ = service.update(&id, |v| {
                    v.observation_error = Some(format!("reconciliation required: {e:#}"))
                });
                eprintln!("fabric: invocation {id} needs reconciliation: {e:#}")
            }
        });
    }
    pub fn resume(self: &Arc<Self>) -> Result<()> {
        let ids: Vec<String> = {
            let db = self.db.lock().unwrap();
            let mut s = db.prepare("SELECT id FROM invocations WHERE active=1")?;
            let rows = s.query_map([], |r| r.get(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        for id in ids {
            self.spawn(id)
        }
        Ok(())
    }
    pub fn cancel(&self, id: &str, caller: &str) -> Result<Invocation> {
        self.get(id, caller)?;
        self.update(id, |v| {
            if !terminal(&v.state) {
                v.state = "cancelling".into()
            }
        })
    }
    pub(crate) fn read_internal(&self, id: &str) -> Result<Invocation> {
        Self::load(&self.db.lock().unwrap(), id)
    }
    fn recover_session(&self, v: &Invocation) -> Result<Option<Running>> {
        let sessions = self.executor.controls.call("GET", "/sessions", None)?;
        let found = sessions
            .as_array()
            .context("invalid session list")?
            .iter()
            .find(|s| s["name"] == format!("fabric:{}", v.id));
        let Some(session) = found else {
            return Ok(None);
        };
        let path = self.executor.root.join(&v.id).join("context.json");
        let context: Value = serde_json::from_slice(&fs::read(path)?)?;
        Ok(Some(Running {
            id: v.id.clone(),
            session_id: session["id"].as_str().context("missing session id")?.into(),
            thread_id: context["thread_id"]
                .as_str()
                .context("missing thread")?
                .into(),
            worker_id: context["connection"]["participant_id"]
                .as_str()
                .context("missing worker")?
                .into(),
            package: v.package.clone(),
            capability: v.capability.clone(),
        }))
    }
    fn drive(&self, id: &str) -> Result<()> {
        let initial = self.read_internal(id)?;
        if initial.external.is_some() {
            return self.drive_external(id);
        }
        if initial.state == "accepted" {
            // Persist before launch: never automatically repeat an uncertain POST.
            let launching = self.update(id, |v| {
                if v.state == "accepted" {
                    v.state = "launching".into()
                }
            })?;
            if launching.state == "launching" {
                let worker = self.executor.controls.register(
                    &self.executor.scope,
                    id,
                    &launching.package.name,
                )?;
                self.update(id, |v| v.assigned_peer = Some(worker.participant_id))?;
                match self.executor.launch(
                    id,
                    &launching.package,
                    &launching.capability,
                    &launching.request.input,
                    Some(&launching.caller),
                ) {
                    Ok(running) => {
                        self.update(id, |v| {
                            v.running = Some(running);
                            if v.state == "launching" {
                                v.state = "running".into()
                            }
                        })?;
                    }
                    Err(e) => {
                        self.update(id, |v| {
                            v.state = if e.is::<LaunchUncertain>() {
                                "launch_uncertain"
                            } else {
                                "failed"
                            }
                            .into();
                            v.error = Some(format!("{e:#}; launch was not retried"))
                        })?;
                    }
                }
            }
        }
        loop {
            let v = self.read_internal(id)?;
            if terminal(&v.state) {
                return Ok(());
            }
            let running = if let Some(r) = v.running.clone() {
                r
            } else {
                match self.recover_session(&v) {
                    Ok(Some(r)) => {
                        self.update(id, |v| {
                            v.running = Some(r.clone());
                            if matches!(v.state.as_str(), "launching" | "launch_uncertain") {
                                v.state = "running".into();
                                v.error = None
                            }
                        })?;
                        r
                    }
                    Ok(None) if v.state == "cancelling" => {
                        // A cancellation before launch is definite; an uncertain
                        // launch remains reserved until the host proves its outcome.
                        if v.error.is_none() {
                            self.update(id, |v| v.state = "cancelled".into())?;
                            return Ok(());
                        }
                        std::thread::sleep(Duration::from_secs(1));
                        continue;
                    }
                    _ => {
                        std::thread::sleep(Duration::from_secs(1));
                        continue;
                    }
                }
            };
            let attempt = (|| -> Result<bool> {
                if matches!(v.state.as_str(), "cancelling" | "finishing") {
                    self.executor.stop(&running)?;
                } else {
                    let mut cursor = v.cursor.clone();
                    let result = self.executor.poll_result(&running, &mut cursor);
                    if result.as_ref().is_err_and(|e| e.is::<reqwest::Error>()) {
                        return Err(result.unwrap_err());
                    }
                    self.update(id, |v| {
                        v.cursor = cursor;
                        if v.state == "running" {
                            match result {
                                Ok(Some(output)) => {
                                    v.output = Some(output);
                                    v.state = "finishing".into()
                                }
                                Err(ref e) => {
                                    v.error = Some(format!("{e:#}"));
                                    v.state = "finishing".into()
                                }
                                _ => {}
                            }
                        }
                    })?;
                }
                let session = self.executor.session(&running)?;
                if session["state"] != "running" && session["state"] != "stopping" {
                    // Re-read after observing exit so a final committed message
                    // cannot be mistaken for an exit without a result.
                    let latest = self.read_internal(id)?;
                    let mut cursor = latest.cursor.clone();
                    let final_result = self.executor.poll_result(&running, &mut cursor);
                    if final_result
                        .as_ref()
                        .is_err_and(|e| e.is::<reqwest::Error>())
                    {
                        return Err(final_result.unwrap_err());
                    }
                    self.update(id, |v| {
                        v.cursor = cursor;
                        if v.state == "cancelling" {
                            v.state = "cancelled".into();
                            return;
                        }
                        if v.state == "running" {
                            match final_result {
                                Ok(Some(out)) => v.output = Some(out),
                                Err(e) => v.error = Some(format!("{e:#}")),
                                _ => {}
                            }
                        }
                        if v.output.is_some() && v.error.is_none() {
                            v.state = "completed".into()
                        } else {
                            v.state = "failed".into();
                            if v.error.is_none() {
                                v.error = Some(format!(
                                    "session {} without accepted result; exit_code={}",
                                    session["state"], session["exit_code"]
                                ))
                            }
                        }
                    })?;
                    self.executor.controls.retire(&running.worker_id)?;
                    return Ok(true);
                }
                if now().saturating_sub(v.accepted_at) >= v.package.elapsed_seconds {
                    self.update(id, |v| {
                        if v.state == "running" {
                            v.state = "finishing".into();
                            v.error = Some("elapsed time limit exceeded".into())
                        }
                    })?;
                }
                Ok(false)
            })();
            match attempt {
                Ok(true) => return Ok(()),
                Err(e) => {
                    self.update(id, |v| {
                        if !terminal(&v.state) {
                            v.observation_error = Some(format!(
                                "transport unavailable; reconciliation pending: {e:#}"
                            ))
                        }
                    })?;
                }
                _ => {
                    self.update(id, |v| v.observation_error = None)?;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    pub fn authenticate(&self, token: &str) -> Result<String> {
        let peer = self.executor.controls.call(
            "POST",
            "/comms/authenticate",
            Some(&json!({"token":token})),
        )?;
        if peer["execution_id"] != self.executor.scope {
            bail!("not_authorized: identity belongs to another Fabric scope")
        }
        Ok(peer["id"].as_str().context("missing peer id")?.into())
    }
    fn caller_comms(&self, caller: &str, token: &str) -> Result<crate::controls::Comms> {
        let endpoint = self.executor.controls.call("GET", "/comms", None)?;
        crate::controls::Comms::new(crate::controls::Connection {
            url: endpoint["url"]
                .as_str()
                .context("missing comms URL")?
                .into(),
            token: token.into(),
            participant_id: caller.into(),
        })
    }
    pub fn rpc(self: &Arc<Self>, name: &str, args: Value, caller: &str) -> Result<Value> {
        match name {
            "invoke" => Ok(serde_json::to_value(
                self.submit(caller, serde_json::from_value(args)?)?,
            )?),
            "get" => Ok(serde_json::to_value(
                self.get(args["id"].as_str().context("id required")?, caller)?,
            )?),
            "cancel" => Ok(serde_json::to_value(
                self.cancel(args["id"].as_str().context("id required")?, caller)?,
            )?),
            "wait" => {
                let id = args["id"].as_str().context("id required")?;
                let seconds = args
                    .get("seconds")
                    .map(|v| v.as_u64().context("seconds must be an integer"))
                    .transpose()?
                    .unwrap_or(20);
                if seconds > 30 {
                    bail!("seconds must be 0..30")
                }
                let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
                loop {
                    let v = self.get(id, caller)?;
                    if terminal(&v.state) || std::time::Instant::now() >= deadline {
                        return Ok(serde_json::to_value(v)?);
                    };
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            "catalog" => self.catalog(),
            "describe" => {
                let catalog = self.catalog()?;
                let found = catalog
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|v| {
                        v["package"] == args["package"]
                            && (args.get("capability").is_none()
                                || v["capability"]["name"] == args["capability"])
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if found.is_empty() {
                    bail!("unknown capability")
                };
                Ok(json!(found))
            }
            "register" => Ok(serde_json::to_value(
                self.register(caller, serde_json::from_value(args)?)?,
            )?),
            "withdraw" => {
                self.withdraw(caller, args["name"].as_str().context("name required")?)?;
                Ok(json!({"withdrawn":true}))
            }
            _ => bail!("unknown Fabric operation"),
        }
    }
}

pub fn serve(config: Config, root: &Path) -> Result<()> {
    let service = Service::open(config, root)?;
    let socket = root.join("fabric.sock");
    if socket.exists() {
        fs::remove_file(&socket)?
    }
    let server = tiny_http::Server::http_unix(&socket).map_err(|e| anyhow::anyhow!("{e}"))?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))?;
    service.resume()?;
    eprintln!("Fabric listening on {}", socket.display());
    while !stopped.load(Ordering::Relaxed) {
        let Some(mut request) = server.recv_timeout(Duration::from_millis(100))? else {
            continue;
        };
        let service = service.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Value> {
                if request.method().as_str() != "POST" || request.url() != "/rpc" {
                    bail!("POST /rpc required")
                }
                let mut raw = String::new();
                request.as_reader().take(131073).read_to_string(&mut raw)?;
                if raw.len() > 131072 {
                    bail!("request too large")
                }
                let body: Value = serde_json::from_str(&raw)?;
                let method = body["method"].as_str().context("method required")?;
                let params = body.get("params").cloned().unwrap_or(json!({}));
                if method == "attach" {
                    let key = params["key"].as_str().context("key required")?;
                    if key.len() > 150 {
                        bail!("identity key too long")
                    }
                    let connection = service.executor.controls.register(
                        &service.executor.scope,
                        &format!("operator:{key}"),
                        params["name"].as_str().unwrap_or(key),
                    )?;
                    service.db.lock().unwrap().execute(
                        "INSERT OR IGNORE INTO metadata(key,value) VALUES(?1,'operator')",
                        [format!("operator:{}", connection.participant_id)],
                    )?;
                    return Ok(serde_json::to_value(connection)?);
                }
                let token = body["token"]
                    .as_str()
                    .context("participant token required")?;
                let caller = service.authenticate(token)?;
                if method == "comms_tools" {
                    return Ok(service
                        .caller_comms(&caller, token)?
                        .rpc("tools/list", json!({}))?["tools"]
                        .clone());
                }
                if method == "comms_call" {
                    let name = params["name"]
                        .as_str()
                        .context("comms tool name required")?;
                    if ![
                        "create_thread",
                        "add_participant",
                        "remove_participants",
                        "list_participants",
                        "send_message",
                        "read_messages",
                        "wait_for_message",
                        "close_thread",
                    ]
                    .contains(&name)
                    {
                        bail!("unknown comms tool")
                    }
                    return service
                        .caller_comms(&caller, token)?
                        .call(name, params.get("arguments").cloned().unwrap_or(json!({})));
                }
                service.rpc(method, params, &caller)
            })();
            let value = match result {
                Ok(value) => json!({"result":value}),
                Err(e) => json!({"error":format!("{e:#}")}),
            };
            let response = tiny_http::Response::from_string(value.to_string()).with_header(
                tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
            );
            let _ = request.respond(response);
        });
    }
    fs::remove_file(socket)?;
    Ok(())
}

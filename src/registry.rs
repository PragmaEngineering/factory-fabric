use crate::{
    config::{validate_schema, Capability, Package},
    invocation::Running,
    service::{now, terminal, Service},
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationRequest {
    pub name: String,
    pub capabilities: Vec<Capability>,
    pub lease_seconds: u64,
    #[serde(default)]
    pub allow_delegation: bool,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Registration {
    pub id: String,
    pub owner: String,
    pub request: RegistrationRequest,
    pub mailbox: String,
    pub expires_at: u64,
}
impl Service {
    pub fn register(&self, caller: &str, args: RegistrationRequest) -> Result<Registration> {
        if !crate::config::identifier(&args.name)
            || args.capabilities.is_empty()
            || !(1..=300).contains(&args.lease_seconds)
        {
            bail!("registration requires a name, capabilities, and a lease of 1..300 seconds")
        }
        if self.config.packages.iter().any(|p| p.name == args.name) {
            bail!("configured package already uses this name")
        }
        let mut names = std::collections::BTreeSet::new();
        for c in &args.capabilities {
            if !crate::config::identifier(&c.name)
                || c.description.trim().is_empty()
                || c.version.trim().is_empty()
                || !names.insert(&c.name)
            {
                bail!("invalid or duplicate capability")
            }
            validate_schema(&c.input_schema)?;
            validate_schema(&c.output_schema)?;
        }
        self.peer(caller)?;
        let db = self.db.lock().unwrap();
        let previous = Self::registered(&db, &args.name)?;
        if previous.as_ref().is_some_and(|r| r.owner != caller) {
            bail!("not_authorized: registration belongs to another identity")
        }
        let (id, mailbox) = if let Some(old) = previous {
            (old.id, old.mailbox)
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            let thread=self.executor.comms.call("create_thread",json!({"operation_id":format!("mailbox:{id}"),"title":format!("{} capability inbox",args.name),"participants":[caller]}))?;
            (
                id,
                thread["thread_id"]
                    .as_str()
                    .context("missing mailbox")?
                    .into(),
            )
        };
        let registration = Registration {
            id,
            owner: caller.into(),
            expires_at: now() + args.lease_seconds,
            request: args,
            mailbox,
        };
        db.execute("INSERT INTO registrations(name,body) VALUES(?1,?2) ON CONFLICT(name) DO UPDATE SET body=excluded.body",params![registration.request.name,serde_json::to_string(&registration)?])?;
        Ok(registration)
    }
    pub fn peer(&self, id: &str) -> Result<Value> {
        let peer =
            self.executor
                .controls
                .call("GET", &format!("/comms/participants/{id}"), None)?;
        if peer["active"] != true || peer["execution_id"] != self.executor.scope {
            bail!("not_authorized: active participant in this Fabric scope required")
        }
        Ok(peer)
    }
    pub(crate) fn registered(db: &Connection, name: &str) -> Result<Option<Registration>> {
        let raw: Option<String> = db
            .query_row(
                "SELECT body FROM registrations WHERE name=?1",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn withdraw(&self, caller: &str, name: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        let mut r = Self::registered(&db, name)?.context("unknown registration")?;
        if r.owner != caller {
            bail!("not_authorized: registration belongs to another identity")
        }
        r.expires_at = 0;
        db.execute(
            "UPDATE registrations SET body=?2 WHERE name=?1",
            params![name, serde_json::to_string(&r)?],
        )?;
        Ok(())
    }
    pub fn catalog(&self) -> Result<Value> {
        let registrations: Vec<Registration> = {
            let db = self.db.lock().unwrap();
            let mut q = db.prepare("SELECT body FROM registrations ORDER BY name")?;
            let rows = q.query_map([], |r| r.get::<_, String>(0))?;
            rows.map(|s| Ok(serde_json::from_str(&s?)?))
                .collect::<Result<_>>()?
        };
        let mut catalog = self.config.catalog().as_array().unwrap().clone();
        for r in registrations {
            let availability = if r.expires_at <= now() {
                "offline"
            } else if self.peer(&r.owner).is_ok() {
                "connected"
            } else {
                "unknown"
            };
            for c in &r.request.capabilities {
                catalog.push(json!({"package":r.request.name,"capability":c,"availability":availability,"registration_id":r.id,"expires_at":r.expires_at}));
            }
        }
        Ok(Value::Array(catalog))
    }
    pub(crate) fn registered_package(
        &self,
        db: &Connection,
        name: &str,
    ) -> Result<(Package, Registration)> {
        let r = Self::registered(db, name)?.context("unknown package")?;
        if r.expires_at <= now() {
            bail!("unavailable: registration lease expired or withdrawn")
        }
        self.peer(&r.owner)?;
        let p = Package {
            name: r.request.name.clone(),
            path: PathBuf::new(),
            entrypoint: PathBuf::new(),
            args: vec![],
            overrides: BTreeMap::new(),
            environment: BTreeMap::new(),
            working_directory: None,
            elapsed_seconds: 1800,
            allow_delegation: r.request.allow_delegation,
            capabilities: r.request.capabilities.clone(),
        };
        Ok((p, r))
    }
    pub(crate) fn drive_external(&self, id: &str) -> Result<()> {
        let initial = self.read_internal(id)?;
        let registration = initial.external.clone().context("missing registration")?;
        let running = if let Some(r) = initial.running {
            r
        } else {
            let thread=self.executor.comms.call("create_thread",json!({"operation_id":format!("thread:{id}"),"title":format!("{}.{} {id}",initial.package.name,initial.capability.name),"participants":[registration.owner,initial.caller]}))?;
            let running = Running {
                id: id.into(),
                session_id: String::new(),
                thread_id: thread["thread_id"]
                    .as_str()
                    .context("missing thread")?
                    .into(),
                worker_id: registration.owner.clone(),
                package: initial.package,
                capability: initial.capability,
            };
            self.update(id, |v| {
                v.running = Some(running.clone());
                if v.state == "accepted" {
                    v.state = "running".into()
                }
            })?;
            running
        };
        let invocation = self.read_internal(id)?;
        if invocation.state != "cancelling" {
            self.executor.comms.call("send_message",json!({"operation_id":format!("dispatch:{id}"),"thread_id":registration.mailbox,"recipients":[registration.owner],"payload":{"kind":"fabric.invoke","version":1,"parts":[{"kind":"data","data":{"invocation_id":id,"capability":invocation.capability,"input":invocation.request.input,"thread_id":running.thread_id,"result_recipient":self.executor.comms.connection.participant_id}}]}}))?;
        }
        let mut cancel_sent = false;
        loop {
            let v = self.read_internal(id)?;
            if terminal(&v.state) {
                return Ok(());
            }
            if v.state == "cancelling" && !cancel_sent {
                self.executor.comms.call("send_message",json!({"operation_id":format!("cancel:{id}"),"thread_id":registration.mailbox,"recipients":[registration.owner],"payload":{"kind":"fabric.cancel","version":1,"parts":[{"kind":"data","data":{"invocation_id":id,"thread_id":running.thread_id}}]}}))?;
                cancel_sent = true;
            }
            let mut cursor = v.cursor.clone();
            let result = self.executor.poll_result(&running, &mut cursor);
            if result.as_ref().is_err_and(|e| e.is::<reqwest::Error>()) {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            let mut done = false;
            self.update(id, |v| {
                v.cursor = cursor;
                match result {
                    Ok(Some(out)) => {
                        if v.state == "cancelling" {
                            v.state = "cancelled".into()
                        } else {
                            v.state = "completed".into();
                            v.output = Some(out)
                        };
                        done = true
                    }
                    Err(e) => {
                        v.state = if v.state == "cancelling" {
                            "cancelled"
                        } else {
                            "failed"
                        }
                        .into();
                        v.error = Some(format!("{e:#}"));
                        done = true
                    }
                    _ => {}
                }
            })?;
            if done {
                return Ok(());
            }
            let current = {
                let db = self.db.lock().unwrap();
                Self::registered(&db, &registration.request.name)?
            };
            if current.is_none_or(|r| r.id != registration.id || r.expires_at <= now())
                || now().saturating_sub(v.accepted_at) >= v.package.elapsed_seconds
            {
                self.update(id,|v|{v.state="failed".into();v.error=Some("external implementation lease or invocation expired; process state unknown".into())})?;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

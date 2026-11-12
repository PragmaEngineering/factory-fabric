use crate::controls::{Connection, Controls};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone)]
pub struct Client {
    host: Controls,
    pub connection: Connection,
}
impl Client {
    /// Compatibility wrapper for existing local callers.
    pub fn attach(socket: &Path, key: &str) -> Result<Self> {
        Self::attach_endpoint(&socket.to_string_lossy(), key)
    }
    pub fn attach_endpoint(endpoint: &str, key: &str) -> Result<Self> {
        let host = if endpoint.starts_with("http") {
            Controls::connect(
                endpoint,
                &std::env::var("FABRIC_TOKEN")
                    .context("set FABRIC_TOKEN for network operator access")?,
            )?
        } else {
            Controls::new(Path::new(endpoint))?
        };
        let response = host.call(
            "POST",
            "/rpc",
            Some(&json!({"method":"attach","params":{"key":key,"name":key}})),
        )?;
        if let Some(e) = response.get("error") {
            bail!("{e}")
        }
        let connection = serde_json::from_value(response["result"].clone())?;
        Ok(Self { host, connection })
    }
    pub fn from_context(socket: &Path, path: &Path) -> Result<Self> {
        Self::from_context_endpoint(&socket.to_string_lossy(), path)
    }
    pub fn from_context_endpoint(endpoint: &str, path: &Path) -> Result<Self> {
        let context: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let connection: Connection = serde_json::from_value(
            context
                .get("connection")
                .cloned()
                .context("context missing connection")?,
        )?;
        let endpoint = context["fabric_endpoint"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| endpoint.to_owned());
        let host = if endpoint.starts_with("http") {
            Controls::connect(&endpoint, &connection.token)?
        } else {
            Controls::new(Path::new(&endpoint))?
        };
        Ok(Self { host, connection })
    }
    pub fn from_assignment(endpoint: &str) -> Result<Self> {
        let url =
            std::env::var("AGENT_MCP_URL").context("assigned adapter requires AGENT_MCP_URL")?;
        let token = std::env::var("AGENT_MCP_TOKEN")
            .context("assigned adapter requires AGENT_MCP_TOKEN")?;
        let mut connection = Connection {
            url,
            token,
            participant_id: String::new(),
        };
        let assignment =
            crate::controls::Comms::new(connection.clone())?.call("assignment_get", json!({}))?;
        connection.participant_id = assignment["participant_id"]
            .as_str()
            .context("assignment missing participant")?
            .into();
        Ok(Self {
            host: Controls::connect(endpoint, &connection.token)?,
            connection,
        })
    }
    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        let response = self.host.call(
            "POST",
            "/rpc",
            Some(&json!({"method":method,"params":params,"token":self.connection.token})),
        )?;
        if let Some(e) = response.get("error") {
            bail!("{}", e.as_str().unwrap_or("Fabric call failed"))
        }
        response
            .get("result")
            .cloned()
            .context("missing Fabric result")
    }
}

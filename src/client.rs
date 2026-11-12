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
    pub fn attach(socket: &Path, key: &str) -> Result<Self> {
        let host = Controls::new(socket)?;
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
        let context: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        Ok(Self {
            host: Controls::new(socket)?,
            connection: serde_json::from_value(
                context
                    .get("connection")
                    .cloned()
                    .context("context missing connection")?,
            )?,
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

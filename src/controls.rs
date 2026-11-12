use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

#[derive(Clone)]
pub struct Controls {
    http: Client,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Connection {
    pub url: String,
    pub token: String,
    pub participant_id: String,
}
impl Controls {
    pub fn new(socket: &Path) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .unix_socket(socket)
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()?,
        })
    }
    pub fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut request = self
            .http
            .request(method.parse()?, format!("http://localhost{path}"));
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .context("Controls request failed; inspect session state before retrying a launch")?;
        let status = response.status();
        let value: Value = response.json()?;
        if !status.is_success() {
            bail!("Controls {path}: {value}");
        }
        Ok(value)
    }
    pub fn scope(&self, name: &str) -> Result<String> {
        let v = self.call("POST", "/comms/scopes", Some(&json!({"name":name})))?;
        Ok(v["id"].as_str().context("missing scope id")?.to_string())
    }
    pub fn register(&self, scope: &str, key: &str, name: &str) -> Result<Connection> {
        Ok(serde_json::from_value(self.call(
            "POST",
            &format!("/comms/scopes/{scope}/participants"),
            Some(&json!({"key":key,"name":name})),
        )?)?)
    }
    pub fn retire(&self, id: &str) -> Result<()> {
        self.call("DELETE", &format!("/comms/participants/{id}"), None)?;
        Ok(())
    }
}

/// Minimal Streamable HTTP client for the existing stateless comms endpoint.
/// Messaging semantics and persistence remain entirely in Controls.
#[derive(Clone)]
pub struct Comms {
    http: Client,
    pub connection: Connection,
}
impl Comms {
    pub fn new(connection: Connection) -> Result<Self> {
        let url = reqwest::Url::parse(&connection.url)?;
        if url.scheme() != "http"
            || !url.host_str().is_some_and(|h| {
                h.parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
            })
        {
            bail!("Controls comms endpoint must be loopback HTTP");
        }
        let client = Self {
            http: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(40))
                .build()?,
            connection,
        };
        client.rpc("initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"fabric","version":env!("CARGO_PKG_VERSION")}}))?;
        Ok(client)
    }
    pub(crate) fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        let response = self
            .http
            .post(&self.connection.url)
            .bearer_auth(&self.connection.token)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-03-26")
            .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .send()?;
        let status = response.status();
        let text = response.text()?;
        if !status.is_success() {
            bail!("comms HTTP {status}");
        }
        let reply: Value = if let Ok(v) = serde_json::from_str(&text) {
            v
        } else {
            text.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
                .find(|v| v["id"] == id)
                .context("missing MCP response")?
        };
        if let Some(e) = reply.get("error") {
            bail!("MCP error: {e}");
        }
        reply.get("result").cloned().context("missing MCP result")
    }
    pub fn call(&self, name: &str, args: Value) -> Result<Value> {
        let result = self.rpc("tools/call", json!({"name":name,"arguments":args}))?;
        if result["isError"] == true {
            bail!("comms rejected {name}: {}", result["content"]);
        }
        if let Some(v) = result.get("structuredContent") {
            return Ok(v.clone());
        }
        let text = result["content"]
            .as_array()
            .and_then(|a| a.iter().find_map(|p| p["text"].as_str()))
            .context("missing tool result")?;
        Ok(serde_json::from_str(text)?)
    }
}

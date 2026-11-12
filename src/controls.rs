use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

#[derive(Clone)]
pub struct Controls {
    http: Client,
    base: String,
    token: Option<String>,
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
            base: "http://localhost".into(),
            token: None,
            http: Client::builder()
                .unix_socket(socket)
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()?,
        })
    }
    pub fn connect(endpoint: &str, token: &str) -> Result<Self> {
        validate_origin(endpoint)?;
        if token.trim().len() < 32 {
            bail!("network credential must contain at least 32 characters")
        }
        let mut builder = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(40));
        if let Ok(path) = std::env::var("FABRIC_CA_CERT") {
            builder = builder
                .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(path)?)?);
        }
        Ok(Self {
            http: builder.build()?,
            base: endpoint.trim_end_matches('/').into(),
            token: Some(token.trim().into()),
        })
    }
    pub fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut request = self
            .http
            .request(method.parse()?, format!("{}{path}", self.base));
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .context("Controls request failed; inspect session state before retrying a launch")?;
        let status = response.status();
        if !status.is_success() {
            bail!("Controls {path}: HTTP {status}: {}", response.text()?);
        }
        let value: Value = response.json()?;
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
        validate_endpoint(&connection.url)?;
        let mut builder = Client::builder();
        if let Ok(path) = std::env::var("FABRIC_CA_CERT") {
            builder = builder
                .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(path)?)?);
        }
        let client = Self {
            http: builder
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

pub fn validate_endpoint(endpoint: &str) -> Result<()> {
    let url = reqwest::Url::parse(endpoint)?;
    let loopback = url.host_str().is_some_and(|h| {
        h == "localhost"
            || h.parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("endpoint requires HTTPS (HTTP allowed only on loopback), without embedded credentials, query, or fragment")
    }
    Ok(())
}

pub fn validate_origin(endpoint: &str) -> Result<()> {
    validate_endpoint(endpoint)?;
    if reqwest::Url::parse(endpoint)?.path() != "/" {
        bail!("service endpoint must be an origin URL without a path")
    }
    Ok(())
}

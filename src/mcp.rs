//! Stdio MCP adapter. Capability tools remain generic; communication tool
//! descriptions and schemas are obtained from Controls rather than duplicated.
use crate::{client::Client, config::validate_value};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({"name":name,"description":description,"inputSchema":schema})
}
pub fn tools(client: &Client) -> Result<Vec<Value>> {
    let id = object(json!({"id":{"type":"string"}}), &["id"]);
    let mut tools=vec![
        tool("agents_list","Discover configured and connected capabilities, contracts and availability.",object(json!({}),&[])),
        tool("agents_describe","Inspect a package's capability contracts before invoking.",object(json!({"package":{"type":"string"},"capability":{"type":"string"}}),&["package"])),
        tool("agents_invoke","Submit structured work asynchronously. Reuse request_key only for unchanged retries. Capacity exhaustion rejects unaccepted work.",object(json!({"request_key":{"type":"string","minLength":1,"maxLength":128},"package":{"type":"string"},"capability":{"type":"string"},"input":{},"working_directory":{"type":"string"},"parent_id":{"type":"string"}}),&["request_key","package","capability","input"])),
        tool("invocations_get","Retrieve invocation status and a validated result when complete.",id.clone()),
        tool("invocations_cancel","Request cancellation; inspect status for confirmation.",id),
        tool("invocations_wait","Wait up to 30 seconds for a terminal result; returns current status on timeout.",object(json!({"id":{"type":"string"},"seconds":{"type":"integer","minimum":0,"maximum":30}}),&["id"])),
        tool("agents_register","Publish capabilities implemented by this connected identity. Renew the lease by registering again; consume the returned comms mailbox.",object(json!({"name":{"type":"string"},"capabilities":{"type":"array","minItems":1,"items":object(json!({"name":{"type":"string"},"description":{"type":"string"},"version":{"type":"string"},"input_schema":{},"output_schema":{}}),&["name","description","version","input_schema","output_schema"])},"lease_seconds":{"type":"integer","minimum":1,"maximum":300},"allow_delegation":{"type":"boolean"}}),&["name","capabilities","lease_seconds"])),
        tool("agents_withdraw","Withdraw this identity's capability registration.",object(json!({"name":{"type":"string"}}),&["name"])),
    ];
    let canonical = client.call("comms_tools", json!({}))?;
    for mut t in canonical
        .as_array()
        .context("missing comms tool definitions")?
        .clone()
    {
        let name = t["name"].as_str().context("missing comms tool name")?;
        t["name"] = json!(format!("comms_{name}"));
        tools.push(t);
    }
    Ok(tools)
}
fn call(client: &Client, name: &str, args: Value) -> Result<Value> {
    let definitions = tools(client)?;
    let definition = definitions
        .iter()
        .find(|t| t["name"] == name)
        .context("unknown tool")?;
    validate_value(&definition["inputSchema"], &args)?;
    if let Some(name) = name.strip_prefix("comms_") {
        return client.call("comms_call", json!({"name":name,"arguments":args}));
    }
    let operation = match name {
        "agents_list" => "catalog",
        "agents_describe" => "describe",
        "agents_invoke" => "invoke",
        "invocations_get" => "get",
        "invocations_cancel" => "cancel",
        "invocations_wait" => "wait",
        "agents_register" => "register",
        "agents_withdraw" => "withdraw",
        _ => bail!("unknown tool"),
    };
    client.call(operation, args)
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
pub fn serve(client: Client) -> Result<()> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut output = std::io::stdout().lock();
    let mut initialized = false;
    loop {
        let mut raw = Vec::new();
        let count = (&mut input).take(1048577).read_until(b'\n', &mut raw)?;
        if count == 0 {
            break;
        }
        if raw.len() > 1048576 {
            bail!("MCP request exceeds 1 MiB")
        }
        let reply = match serde_json::from_slice::<Value>(&raw) {
            Err(_) => Some(error(Value::Null, -32700, "Parse error")),
            Ok(request) => {
                if request["jsonrpc"] != "2.0" {
                    Some(error(
                        request.get("id").cloned().unwrap_or(Value::Null),
                        -32600,
                        "Invalid Request",
                    ))
                } else if !request.as_object().is_some_and(|r| r.contains_key("id")) {
                    None
                } else {
                    let id = request["id"].clone();
                    let result = match request["method"].as_str() {
                        Some("initialize") => {
                            initialized = true;
                            Ok(
                                json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"agent-fabric","version":env!("CARGO_PKG_VERSION")},"instructions":"Discover capabilities, inspect contracts, and delegate independent work. Submission is asynchronous. Use existing comms tools for follow-up messages; workflow control payloads are unavailable."}),
                            )
                        }
                        Some("ping") => Ok(json!({})),
                        _ if !initialized => {
                            let reply = error(id, -32000, "Initialize first");
                            writeln!(output, "{reply}")?;
                            output.flush()?;
                            continue;
                        }
                        Some("tools/list") => tools(&client).map(|tools| json!({"tools":tools})),
                        Some("tools/call") => {
                            let params = &request["params"];
                            match call(
                                &client,
                                params["name"].as_str().unwrap_or(""),
                                params.get("arguments").cloned().unwrap_or(json!({})),
                            ) {
                                Ok(value) => Ok(
                                    json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":{"result":value},"isError":false}),
                                ),
                                Err(e) => Ok(
                                    json!({"content":[{"type":"text","text":format!("{e:#}")}],"isError":true}),
                                ),
                            }
                        }
                        _ => {
                            let reply = error(id, -32601, "Method not found");
                            writeln!(output, "{reply}")?;
                            output.flush()?;
                            continue;
                        }
                    };
                    Some(match result {
                        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
                        Err(e) => error(id, -32603, &format!("{e:#}")),
                    })
                }
            }
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?
        }
    }
    Ok(())
}

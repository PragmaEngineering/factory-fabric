use agent_fabric::config::Config;
use anyhow::{bail, Result};
use std::path::Path;
const USAGE: &str = "Usage:
  fabric up [fabric.yaml]                      Start the local stack; Ctrl-C stops it
  fabric validate [fabric.yaml]
  fabric catalog [fabric.yaml]                  Inspect configured capabilities offline
  fabric serve CONFIG STATE_DIR                Run the asynchronous service
  fabric call [SOCKET] METHOD ARGUMENTS_JSON     Call as the local CLI identity
  fabric mcp [SOCKET] [--identity NAME | --context PATH]

Defaults use .fabric/fabric.sock in the current directory. See docs/operations.md.";
fn run() -> Result<()> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    if (args.first().is_some_and(|a| a == "call") && args.len() == 3)
        || (args.first().is_some_and(|a| a == "mcp")
            && (args.len() == 1 || args.get(1).is_some_and(|a| a.starts_with("--"))))
    {
        args.insert(1, ".fabric/fabric.sock".into());
    }
    match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => println!("{USAGE}"),
        Some("up") if args.len() <= 2 => {
            agent_fabric::local::up(Path::new(
                args.get(1).map(String::as_str).unwrap_or("fabric.yaml"),
            ))?;
        }
        Some("validate" | "catalog") if args.len() <= 2 => {
            let config = Config::load(Path::new(
                args.get(1).map(String::as_str).unwrap_or("fabric.yaml"),
            ))?;
            if args[0] == "catalog" {
                println!("{}", serde_json::to_string_pretty(&config.catalog())?);
            } else {
                println!("Valid Fabric configuration: {}", config.name);
            }
        }
        Some("serve") if args.len() == 3 => {
            agent_fabric::service::serve(Config::load(Path::new(&args[1]))?, Path::new(&args[2]))?;
        }
        Some("call") if args.len() == 4 => {
            let socket = Path::new(&args[1]);
            let client = if let Ok(path) = std::env::var("FABRIC_CONTEXT_PATH") {
                agent_fabric::client::Client::from_context(socket, Path::new(&path))?
            } else {
                agent_fabric::client::Client::attach(socket, "cli")?
            };
            println!(
                "{}",
                client.call(&args[2], serde_json::from_str(&args[3])?)?
            );
        }
        Some("mcp") if args.len() == 2 || args.len() == 4 => {
            let socket = Path::new(&args[1]);
            let client = if args.get(2).is_some_and(|a| a == "--context") {
                agent_fabric::client::Client::from_context(socket, Path::new(&args[3]))?
            } else if args.get(2).is_some_and(|a| a == "--identity") {
                agent_fabric::client::Client::attach(socket, &args[3])?
            } else if args.len() == 2 {
                agent_fabric::client::Client::attach(
                    socket,
                    &format!("session-{}", uuid::Uuid::new_v4()),
                )?
            } else {
                bail!("usage: fabric mcp SOCKET [--identity NAME | --context PATH]")
            };
            agent_fabric::mcp::serve(client)?;
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("fabric: {e:#}");
        std::process::exit(1);
    }
}

use agent_fabric::{config::Config, invocation::Executor};
use anyhow::{bail, Result};
use std::path::Path;
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
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
            agent_fabric::service::serve(Config::load(Path::new(&args[1]))?,Path::new(&args[2]))?;
        }
        Some("call") if args.len() == 4 => {
            let socket=Path::new(&args[1]);
            let client=if let Ok(path)=std::env::var("FABRIC_CONTEXT_PATH") {agent_fabric::client::Client::from_context(socket,Path::new(&path))?}else{agent_fabric::client::Client::attach(socket,"cli")?};
            println!("{}",client.call(&args[2],serde_json::from_str(&args[3])?)?);
        }
        Some("mcp") if args.len()==2 || args.len()==4 => {
            let socket=Path::new(&args[1]);
            let client=if args.get(2).is_some_and(|a|a=="--context") {
                agent_fabric::client::Client::from_context(socket,Path::new(&args[3]))?
            }else if args.get(2).is_some_and(|a|a=="--identity"){
                agent_fabric::client::Client::attach(socket,&args[3])?
            }else if args.len()==2{
                agent_fabric::client::Client::attach(socket,&format!("session-{}",uuid::Uuid::new_v4()))?
            }else{bail!("usage: fabric mcp SOCKET [--identity NAME | --context PATH]")};
            agent_fabric::mcp::serve(client)?;
        }
        Some("run") if args.len() == 6 => {
            let config = Config::load(Path::new(&args[1]))?;
            let package = config.packages.iter().find(|p| p.name == args[2]).ok_or_else(|| anyhow::anyhow!("unknown package"))?;
            let capability = package.capabilities.iter().find(|c| c.name == args[3]).ok_or_else(|| anyhow::anyhow!("unknown capability"))?;
            let input = serde_json::from_str(&args[4])?;
            let executor = Executor::new(&config, Path::new(&args[5]))?;
            let running = executor.launch(&uuid::Uuid::new_v4().to_string(),package,capability,&input,None)?;
            println!("{}", executor.wait(&running)?);
        }
        _ => bail!("usage: fabric <validate|catalog> [fabric.yaml] | fabric run CONFIG PACKAGE CAPABILITY INPUT_JSON STATE_DIR"),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("fabric: {e:#}");
        std::process::exit(1);
    }
}

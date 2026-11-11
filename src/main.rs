use agent_fabric::config::Config;
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
        _ => bail!("usage: fabric <validate|catalog> [fabric.yaml]"),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("fabric: {e:#}");
        std::process::exit(1);
    }
}

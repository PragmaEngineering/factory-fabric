use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub name: String,
    #[serde(default = "local_socket")]
    pub controls_socket: PathBuf,
    #[serde(default)]
    pub controls_endpoint: Option<String>,
    #[serde(default)]
    pub controls_token_env: Option<String>,
    #[serde(default)]
    pub controls_identity: Option<String>,
    #[serde(default)]
    pub fabric_endpoint: Option<String>,
    #[serde(default = "capacity")]
    pub concurrency: usize,
    pub packages: Vec<Package>,
}
fn local_socket() -> PathBuf {
    PathBuf::from(".fabric/controls/factory.sock")
}
fn capacity() -> usize {
    4
}
fn timeout() -> u64 {
    1800
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub path: PathBuf,
    pub entrypoint: PathBuf,
    #[serde(default = "legacy_protocol")]
    pub protocol: String,
    #[serde(default)]
    pub runtime: Option<NativeRuntime>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub overrides: BTreeMap<String, Value>,
    /// Target environment variable -> source environment variable name.
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    #[serde(default = "timeout")]
    pub elapsed_seconds: u64,
    #[serde(default)]
    pub allow_delegation: bool,
    pub capabilities: Vec<Capability>,
}
fn legacy_protocol() -> String {
    "legacy".into()
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRuntime {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub executable: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub name: String,
    pub description: String,
    pub version: String,
    pub input_schema: Value,
    pub output_schema: Value,
}
pub(crate) fn identifier(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 100
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn refs_local(v: &Value) -> Result<()> {
    match v {
        Value::Object(m) => {
            for key in ["$ref", "$dynamicRef"] {
                if let Some(r) = m.get(key) {
                    if !r.as_str().is_some_and(|s| s.starts_with('#')) {
                        bail!("only local schema references are supported");
                    }
                }
            }
            for v in m.values() {
                refs_local(v)?;
            }
        }
        Value::Array(a) => {
            for v in a {
                refs_local(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub fn validate_schema(v: &Value) -> Result<()> {
    refs_local(v)?;
    if let Some(dialect) = v.get("$schema") {
        if dialect != "https://json-schema.org/draft/2020-12/schema" {
            bail!("schema dialect must be JSON Schema 2020-12");
        }
    }
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(v)
        .map_err(|e| anyhow::anyhow!("invalid schema: {e}"))?;
    Ok(())
}
pub fn validate_value(schema: &Value, value: &Value) -> Result<()> {
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(schema)
        .map_err(|e| anyhow::anyhow!("invalid schema: {e}"))?;
    validator
        .validate(value)
        .map_err(|e| anyhow::anyhow!("contract violation: {e}"))
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let path = fs::canonicalize(path).context("locate fabric.yaml")?;
        let mut config: Self = serde_yaml::from_str(&fs::read_to_string(&path)?)?;
        let base = path.parent().unwrap();
        if config.version != 1
            || !identifier(&config.name)
            || config.concurrency == 0
            || config.packages.is_empty()
        {
            bail!("version must be 1, name must be an identifier, concurrency must be positive, and packages cannot be empty");
        }
        if config.controls_socket.is_relative() {
            config.controls_socket = base.join(&config.controls_socket);
        }
        if let Some(endpoint) = &config.controls_endpoint {
            crate::controls::validate_origin(endpoint)?;
            if config.fabric_endpoint.is_none() {
                bail!("network execution requires fabric_endpoint reachable from the Controls execution host")
            }
        }
        if let Some(endpoint) = &config.fabric_endpoint {
            crate::controls::validate_origin(endpoint)?;
        }
        let mut packages = BTreeSet::new();
        for p in &mut config.packages {
            if !["legacy", "mcp"].contains(&p.protocol.as_str()) {
                bail!("protocol must be legacy or mcp")
            }
            if let Some(runtime) = &p.runtime {
                if runtime.name != "pi" || runtime.version != "=1.0.3" || p.protocol != "mcp" {
                    bail!("native runtime requires protocol: mcp and pi version =1.0.3")
                }
            }

            if !identifier(&p.name) || !packages.insert(p.name.clone()) {
                bail!("invalid or duplicate package name: {}", p.name);
            }
            if p.elapsed_seconds == 0 || p.capabilities.is_empty() {
                bail!("package needs positive elapsed_seconds and capabilities");
            }
            if config.controls_endpoint.is_some() {
                if !p.path.is_absolute()
                    || !p
                        .working_directory
                        .as_ref()
                        .is_some_and(|p| p.is_absolute())
                {
                    bail!("network execution requires absolute execution-host package and working_directory paths")
                }
                if p.entrypoint.is_absolute()
                    || p.entrypoint
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    bail!("entrypoint must stay inside execution-host package")
                }
            } else {
                p.path = fs::canonicalize(base.join(&p.path)).context("locate package path")?;
                if p.entrypoint.is_absolute() {
                    bail!("entrypoint must be relative to its package");
                }
                let entry = fs::canonicalize(p.path.join(&p.entrypoint))
                    .context("locate package entrypoint")?;
                if !entry.starts_with(&p.path) || !entry.is_file() {
                    bail!("entrypoint must be a file inside package path");
                }
                p.working_directory = Some(fs::canonicalize(
                    base.join(p.working_directory.as_deref().unwrap_or(Path::new("."))),
                )?);
                if !p.working_directory.as_ref().unwrap().is_dir() {
                    bail!("working_directory must be a directory");
                }
            }
            for (target, source) in &p.environment {
                let valid = |s: &str| {
                    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                };
                if !valid(target)
                    || !valid(source)
                    || target.starts_with("FABRIC_")
                    || target.starts_with("FACTORY_")
                {
                    bail!("invalid or reserved environment reference: {target}");
                }
            }
            let mut capabilities = BTreeSet::new();
            for c in &p.capabilities {
                if !identifier(&c.name)
                    || !capabilities.insert(c.name.clone())
                    || c.version.trim().is_empty()
                    || c.description.trim().is_empty()
                {
                    bail!("capability needs unique identifier, version and description");
                }
                validate_schema(&c.input_schema)?;
                validate_schema(&c.output_schema)?;
            }
        }
        Ok(config)
    }
    pub fn catalog(&self) -> Value {
        Value::Array(
            self.packages
                .iter()
                .flat_map(|p| {
                    p.capabilities.iter().map(move |c| {
                        serde_json::json!({
                            "package":p.name,"capability":c,"availability":"on_demand"
                        })
                    })
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn contracts_reject_bad_input_and_remote_refs() {
        let schema = json!({"type":"object","required":["diff"],"properties":{"diff":{"type":"string"}},"additionalProperties":false});
        validate_schema(&schema).unwrap();
        validate_value(&schema, &json!({"diff":"patch"})).unwrap();
        assert!(validate_value(&schema, &json!({"diff":42})).is_err());
        assert!(validate_schema(&json!({"$ref":"https://example.com/schema"})).is_err());
        assert!(validate_schema(&json!({"type":"typo"})).is_err());
    }
    #[test]
    fn remote_paths_belong_to_the_execution_host() {
        let c = Config::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fabric.example.yaml"))
            .unwrap();
        let mut remote = c;
        remote.controls_endpoint = Some("https://controls.example.test".into());
        remote.fabric_endpoint = Some("https://fabric.example.test".into());
        remote.packages[0].path = PathBuf::from("/not-on-this-machine/agents");
        remote.packages[0].working_directory = Some(PathBuf::from("/not-on-this-machine/project"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fabric.yaml");
        fs::write(&path, serde_yaml::to_string(&remote).unwrap()).unwrap();
        assert!(Config::load(&path).is_ok());
        remote.packages[0].path = PathBuf::from("relative");
        fs::write(&path, serde_yaml::to_string(&remote).unwrap()).unwrap();
        assert!(Config::load(&path).is_err());
        assert!(crate::controls::validate_endpoint("http://remote.example.test").is_err());
        assert!(
            crate::controls::validate_endpoint("https://user:secret@remote.example.test").is_err()
        );
    }
    #[test]
    fn configuration_rejects_workflows_and_duplicate_packages() {
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("fabric.example.yaml");
        let c = Config::load(&example).unwrap();
        assert_eq!(c.catalog().as_array().unwrap().len(), 2);
        let mut raw: Value = serde_yaml::from_str(&fs::read_to_string(example).unwrap()).unwrap();
        raw["workflow"] = json!({});
        assert!(serde_json::from_value::<Config>(raw).is_err());
        let dir = tempfile::tempdir().unwrap();
        let mut duplicate = c.clone();
        duplicate.packages.push(c.packages[0].clone());
        let path = dir.path().join("fabric.yaml");
        fs::write(&path, serde_yaml::to_string(&duplicate).unwrap()).unwrap();
        assert!(Config::load(&path)
            .unwrap_err()
            .to_string()
            .contains("duplicate"));
    }
}

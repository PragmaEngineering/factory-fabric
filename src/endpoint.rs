use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
pub fn resolve(value: &str) -> Result<String> {
    if value.starts_with("http://") || value.starts_with("https://") {
        return Ok(value.into());
    }
    let path = Path::new(value);
    if path.extension().is_some_and(|e| e == "json") {
        let record: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        if let Some(token_file) = record["token_file"].as_str() {
            if std::env::var_os("FABRIC_CONTEXT_PATH").is_none()
                && std::env::var_os("FABRIC_TOKEN").is_none()
            {
                std::env::set_var(
                    "FABRIC_TOKEN",
                    std::fs::read_to_string(PathBuf::from(token_file))?.trim(),
                );
            }
        }
        return Ok(record["url"]
            .as_str()
            .context("endpoint file missing url")?
            .into());
    }
    Ok(value.into())
}
pub fn credential_matches(expected: &str, actual: &str) -> bool {
    // Compare fixed-size hashes without an early exit.
    use sha2::{Digest, Sha256};
    let a = Sha256::digest(expected.as_bytes());
    let b = Sha256::digest(actual.as_bytes());
    use subtle::ConstantTimeEq;
    bool::from(a.ct_eq(&b))
}

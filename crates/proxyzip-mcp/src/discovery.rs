//! How a second process finds a running ProxyZip's MCP endpoint: `<data>/mcp.json` holds its URL and token.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Endpoint {
    pub url: String,
    pub token: String,
}

pub fn endpoint_file(home: &Path) -> PathBuf {
    home.join("mcp.json")
}

pub fn token_file(home: &Path) -> PathBuf {
    home.join("mcp-token")
}

fn write_private(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// The access token, created on first use. Anyone who can read it can read your captured traffic.
pub fn load_or_create_token(home: &Path) -> Result<String> {
    let file = token_file(home);
    if let Ok(t) = std::fs::read_to_string(&file) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    std::fs::create_dir_all(home)?;
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    write_private(&file, &token)?;
    Ok(token)
}

pub fn regenerate_token(home: &Path) -> Result<String> {
    let _ = std::fs::remove_file(token_file(home));
    load_or_create_token(home)
}

pub fn publish(home: &Path, endpoint: &Endpoint) -> Result<()> {
    write_private(&endpoint_file(home), &serde_json::to_string_pretty(endpoint)?)
}

pub fn unpublish(home: &Path) {
    let _ = std::fs::remove_file(endpoint_file(home));
}

pub fn read(home: &Path) -> Option<Endpoint> {
    serde_json::from_str(&std::fs::read_to_string(endpoint_file(home)).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_stable_and_endpoint_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let a = load_or_create_token(dir.path()).unwrap();
        assert_eq!(a.len(), 64);
        assert_eq!(a, load_or_create_token(dir.path()).unwrap());
        assert_ne!(a, regenerate_token(dir.path()).unwrap());
        let e = Endpoint { url: "http://127.0.0.1:1/mcp".into(), token: a };
        publish(dir.path(), &e).unwrap();
        assert_eq!(read(dir.path()), Some(e));
        unpublish(dir.path());
        assert_eq!(read(dir.path()), None);
    }
}

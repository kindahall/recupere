// Persistent registry of remote `recupere-agent` instances.
//
// Storage layout:
//   - Metadata (id, label, base_url) is persisted as JSON in
//     `<config_dir>/recupere/remote_agents.json`. This file is *not* secret.
//   - Bearer tokens go in the OS keyring under service `recupere-agent`,
//     account `<agent_id>`. The plaintext token never touches disk.
//
// On boot the registry is loaded from disk; on every mutation it is rewritten
// and the keyring is updated to match.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const KEYRING_SERVICE: &str = "recupere-agent";
const METADATA_FILE: &str = "remote_agents.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAgent {
    pub id: String,
    pub label: String,
    /// Base URL of the agent, e.g. `http://127.0.0.1:7878`.
    pub base_url: String,
    /// Bearer token. Stored in the OS keyring; the in-memory copy is the
    /// canonical one for the lifetime of the process. Never serialized to
    /// disk via this struct's Serde derives.
    #[serde(skip_serializing, default)]
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedAgent {
    id: String,
    label: String,
    base_url: String,
}

impl From<&RemoteAgent> for PersistedAgent {
    fn from(agent: &RemoteAgent) -> Self {
        Self {
            id: agent.id.clone(),
            label: agent.label.clone(),
            base_url: agent.base_url.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteAgentSummary {
    pub id: String,
    pub label: String,
    pub base_url: String,
}

impl From<&RemoteAgent> for RemoteAgentSummary {
    fn from(agent: &RemoteAgent) -> Self {
        Self {
            id: agent.id.clone(),
            label: agent.label.clone(),
            base_url: agent.base_url.clone(),
        }
    }
}

fn metadata_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("recupere");
    let _ = fs::create_dir_all(&path);
    path.push(METADATA_FILE);
    Some(path)
}

fn read_token_from_keyring(agent_id: &str) -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, agent_id).ok()?;
    entry.get_password().ok()
}

fn write_token_to_keyring(agent_id: &str, token: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, agent_id)
        .map_err(|error| format!("keyring entry creation failed: {error}"))?;
    entry
        .set_password(token)
        .map_err(|error| format!("keyring write failed: {error}"))
}

fn delete_token_from_keyring(agent_id: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, agent_id) {
        let _ = entry.delete_credential();
    }
}

fn load_persisted_agents() -> Vec<RemoteAgent> {
    let path = match metadata_path() {
        Some(path) => path,
        None => return Vec::new(),
    };
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };
    let persisted: Vec<PersistedAgent> = serde_json::from_str(&raw).unwrap_or_default();
    persisted
        .into_iter()
        .filter_map(|entry| {
            // Skip agents whose token is no longer in the keyring (e.g. user
            // wiped credentials manually). The metadata gets cleaned up on
            // the next save.
            let token = read_token_from_keyring(&entry.id)?;
            Some(RemoteAgent {
                id: entry.id,
                label: entry.label,
                base_url: entry.base_url,
                token,
            })
        })
        .collect()
}

fn save_persisted_agents(agents: &[RemoteAgent]) -> Result<(), String> {
    let path = metadata_path()
        .ok_or_else(|| "unable to resolve a config directory for the agent registry".to_string())?;
    let persisted: Vec<PersistedAgent> = agents.iter().map(PersistedAgent::from).collect();
    let raw = serde_json::to_string_pretty(&persisted)
        .map_err(|error| format!("agent registry serialization failed: {error}"))?;
    fs::write(&path, raw).map_err(|error| format!("agent registry write failed: {error}"))
}

fn registry() -> &'static Mutex<Vec<RemoteAgent>> {
    static REGISTRY: OnceLock<Mutex<Vec<RemoteAgent>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(load_persisted_agents()))
}

pub fn add(agent: RemoteAgent) -> Result<(), String> {
    write_token_to_keyring(&agent.id, &agent.token)?;
    let mut guard = registry().lock().expect("remote agent registry poisoned");
    guard.retain(|existing| {
        if existing.id == agent.id {
            // Stale duplicate — clean up its keyring entry first.
            if existing.token != agent.token {
                let _ = write_token_to_keyring(&existing.id, &agent.token);
            }
            false
        } else {
            true
        }
    });
    guard.push(agent);
    save_persisted_agents(&guard)
}

pub fn remove(id: &str) -> bool {
    let mut guard = registry().lock().expect("remote agent registry poisoned");
    let before = guard.len();
    guard.retain(|existing| existing.id != id);
    let removed = guard.len() != before;
    if removed {
        delete_token_from_keyring(id);
        let _ = save_persisted_agents(&guard);
    }
    removed
}

pub fn get(id: &str) -> Option<RemoteAgent> {
    let guard = registry().lock().expect("remote agent registry poisoned");
    guard.iter().find(|agent| agent.id == id).cloned()
}

pub fn list_summaries() -> Vec<RemoteAgentSummary> {
    let guard = registry().lock().expect("remote agent registry poisoned");
    guard.iter().map(RemoteAgentSummary::from).collect()
}

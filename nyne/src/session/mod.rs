//! Session management — ID generation, state persistence, and registry.
//!
//! This module is the SSOT for:
//! - Session ID generation and validation
//! - Session file I/O (write/read/remove)
//! - Session directory paths (`$XDG_RUNTIME_DIR/nyne/`)
//! - Control socket paths
//! - Active session discovery (registry scan)

/// Session identifier generation and validation.
mod id;
pub mod state;

use std::path::{Path, PathBuf};
use std::{fs, io};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

pub use self::id::SessionId;

/// Persisted daemon metadata for session discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Human-readable session ID.
    pub id: String,
    /// Daemon process PID.
    pub pid: i32,
    /// Canonical mount path the daemon serves.
    pub mount_path: PathBuf,
}

/// Directory for session files: `$XDG_RUNTIME_DIR/nyne/`.
///
/// Falls back to `$XDG_CACHE_HOME/nyne/` if no runtime dir is available.
fn session_dir() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|dirs| dirs.runtime_dir().unwrap_or_else(|| dirs.cache_dir()).join("nyne"))
        .ok_or_else(|| eyre!("cannot determine session directory (no home dir)"))
}

/// Create the session directory if it doesn't exist. Returns the path.
pub fn ensure_session_dir() -> Result<PathBuf> {
    let dir = session_dir()?;
    fs::create_dir_all(&dir).wrap_err_with(|| format!("creating session dir {}", dir.display()))?;
    Ok(dir)
}

/// Session file path: `<session_dir>/<id>.json`.
pub fn session_file(id: &str) -> Result<PathBuf> { session_dir().map(|dir| dir.join(format!("{id}.json"))) }

/// Control socket path: `<session_dir>/<id>.sock`.
pub fn control_socket(id: &str) -> Result<PathBuf> { session_dir().map(|dir| dir.join(format!("{id}.sock"))) }

/// Write a session file for the given session ID and daemon PID.
pub fn write(id: &SessionId, mount_path: &Path, daemon_pid: i32) -> Result<PathBuf> {
    ensure_session_dir()?;
    let path = session_file(id.as_str())?;

    let info = SessionInfo {
        id: id.to_string(),
        pid: daemon_pid,
        mount_path: mount_path.to_path_buf(),
    };

    let json = serde_json::to_string_pretty(&info).wrap_err("serializing session info")?;
    fs::write(&path, json).wrap_err_with(|| format!("writing session file {}", path.display()))?;

    debug!(path = %path.display(), pid = daemon_pid, id = %id, "session file written");
    Ok(path)
}

/// Read and validate a session file. Returns `None` for stale sessions
/// (dead daemon PID), removing the stale file as a side effect.
pub fn read(path: &Path) -> Result<SessionInfo> {
    let json = fs::read_to_string(path).wrap_err_with(|| format!("reading session file {}", path.display()))?;
    let info: SessionInfo = serde_json::from_str(&json).wrap_err("parsing session file")?;

    if !state::is_pid_alive(info.pid) {
        warn!(pid = info.pid, id = %info.id, "stale session (daemon not running), removing");
        remove(path);
        return Err(eyre!("daemon PID {} is not running — stale session removed", info.pid));
    }

    debug!(pid = info.pid, id = %info.id, "session valid");
    Ok(info)
}

/// Remove a session file. Best-effort — logs but doesn't fail.
pub fn remove(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => debug!(path = %path.display(), "session file removed"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => warn!(path = %path.display(), error = %e, "failed to remove session file"),
    }
}

/// Snapshot of all active sessions on this host.
pub struct SessionRegistry {
    sessions: Vec<SessionInfo>,
}

/// Discovery, lookup, and resolution of active sessions.
impl SessionRegistry {
    /// Scan the session directory and load all valid (live daemon) sessions.
    pub(crate) fn scan() -> Result<Self> {
        let dir = match session_dir() {
            Ok(dir) if dir.is_dir() => dir,
            _ => return Ok(Self { sessions: Vec::new() }),
        };

        let mut sessions = Vec::new();
        for entry in fs::read_dir(&dir).wrap_err("reading session directory")? {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, "reading session dir entry");
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(info) = read(&path)
            {
                sessions.push(info);
            }
        }

        Ok(Self { sessions })
    }

    /// Check whether a session ID is currently active.
    pub(crate) fn is_active(&self, id: &str) -> bool { self.sessions.iter().any(|s| s.id == id) }

    /// Look up a session by ID.
    pub(crate) fn get(&self, id: &str) -> Option<&SessionInfo> { self.sessions.iter().find(|s| s.id == id) }

    /// All active sessions.
    pub(crate) fn sessions(&self) -> &[SessionInfo] { &self.sessions }

    /// Resolve a session — by explicit ID, or the only active session.
    ///
    /// Returns an error if no ID is given and there are zero or multiple
    /// active sessions.
    pub(crate) fn resolve(&self, id: Option<&str>) -> Result<&SessionInfo> {
        if let Some(id) = id {
            return self.get(id).ok_or_else(|| eyre!("no active session with ID {id:?}"));
        }

        match self.sessions.len() {
            0 => Err(eyre!("no active nyne sessions")),
            1 => self.sessions.first().ok_or_else(|| eyre!("no active nyne sessions")),
            n => Err(eyre!(
                "{n} active sessions — specify an ID.\nActive sessions: {}",
                self.sessions
                    .iter()
                    .map(|s| s.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;

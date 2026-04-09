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
use std::path::{Path, PathBuf};
use std::{env, fs, io};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

pub use self::id::SessionId;
use crate::sandbox::control::NYNE_SESSION_DIR_ENV;

/// Persisted daemon metadata for session discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionInfo {
    /// Human-readable session ID.
    pub id: SessionId,
    /// Daemon process PID.
    pub pid: i32,
    /// Canonical mount path the daemon serves.
    pub mount_path: PathBuf,
}

/// Directory for session files.
///
/// Resolution order:
///
/// 1. **`NYNE_SESSION_DIR` env var** — set by `nyne attach` in the child
///    environment, pointing to a per-daemon nested dir
///    (`<parent session dir>/<daemon id>.d/`). This scopes nested
///    sessions to their parent sandbox, so every process inside a given
///    attach-chain sees the same session set regardless of which attach
///    it came through.
/// 2. **`$XDG_RUNTIME_DIR/nyne/`** — host default.
/// 3. **`$XDG_CACHE_HOME/nyne/`** — fallback when no runtime dir exists.
fn session_dir() -> Result<PathBuf> {
    if let Some(override_path) = env::var_os(NYNE_SESSION_DIR_ENV) {
        return Ok(PathBuf::from(override_path));
    }
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
/// Directory for nested sessions spawned inside a sandbox attached to `parent_id`.
///
/// Returns `<current session dir>/<parent_id>.d/`. `nyne attach` creates
/// this directory and exports it via `NYNE_SESSION_DIR` in the child's
/// environment so every nyne invocation inside the attach-chain agrees
/// on the session directory — decoupling session scoping from any
/// namespace identity that differs per attach.
pub fn nested_dir(parent_id: &str) -> Result<PathBuf> {
    session_dir().map(|dir| dir.join(parent_id).with_extension("d"))
}

/// Session file path: `<session_dir>/<id>.json`.
pub fn session_file(id: &str) -> Result<PathBuf> { session_dir().map(|dir| dir.join(id).with_extension("json")) }

/// Control socket path: `<session_dir>/<id>.sock`.
pub fn control_socket(id: &str) -> Result<PathBuf> { session_dir().map(|dir| dir.join(id).with_extension("sock")) }

/// Write a session file for the given session ID and daemon PID.
pub fn write(id: &SessionId, mount_path: &Path, daemon_pid: i32) -> Result<PathBuf> {
    ensure_session_dir()?;
    let path = session_file(id.as_str())?;

    fs::write(
        &path,
        serde_json::to_string_pretty(&SessionInfo {
            id: id.clone(),
            pid: daemon_pid,
            mount_path: mount_path.to_path_buf(),
        })
        .wrap_err("serializing session info")?,
    )
    .wrap_err_with(|| format!("writing session file {}", path.display()))?;

    debug!(path = %path.display(), pid = daemon_pid, id = %id, "session file written");
    Ok(path)
}

/// Read and validate a session file. Returns `Err` for stale sessions
/// (control socket missing), removing the stale file as a side effect.
///
/// Uses the control socket's presence as the liveness signal rather
/// than a PID check: the daemon removes its socket on clean shutdown,
/// and PIDs are meaningless across nested PID namespaces — so a PID
/// check would incorrectly classify live nested daemons as dead.
pub fn read(path: &Path) -> Result<SessionInfo> {
    let info: SessionInfo = serde_json::from_str(
        &fs::read_to_string(path).wrap_err_with(|| format!("reading session file {}", path.display()))?,
    )
    .wrap_err("parsing session file")?;

    let socket_path = control_socket(info.id.as_str())?;
    if !socket_path.exists() {
        warn!(path = %socket_path.display(), id = %info.id, "stale session (control socket missing), removing");
        remove(path);
        return Err(eyre!(
            "control socket {} missing — stale session removed",
            socket_path.display()
        ));
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
            let path = match entry {
                Ok(e) => e.path(),
                Err(e) => {
                    warn!(error = %e, "reading session dir entry");
                    continue;
                }
            };
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(info) = read(&path)
            {
                sessions.push(info);
            }
        }

        Ok(Self { sessions })
    }

    /// Check whether a session ID is currently active.
    pub(crate) fn is_active(&self, id: &str) -> bool { self.sessions.iter().any(|s| s.id.as_str() == id) }

    /// Look up a session by ID.
    pub(crate) fn get(&self, id: &str) -> Option<&SessionInfo> { self.sessions.iter().find(|s| s.id.as_str() == id) }

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

        match self.sessions.as_slice() {
            [] => Err(eyre!("no active nyne sessions")),
            [single] => Ok(single),
            sessions => Err(eyre!(
                "{} active sessions — specify an ID.\nActive sessions: {}",
                sessions.len(),
                sessions.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", ")
            )),
        }
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;

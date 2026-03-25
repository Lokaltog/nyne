//! Control socket — IPC between the daemon and CLI commands.
//!
//! Carries multiple message types over a single Unix domain socket:
//! - **Exec**: dispatch a provider script (used by `nyne exec`)
//! - **Register/Unregister**: track attached processes (used by `nyne attach`)
//! - **`ListProcesses`**: query attached processes (used by `nyne list`)
//!
//! Wire format: newline-delimited JSON, one request → one response per connection.

use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use std::{fs, thread};

use base64::prelude::{BASE64_STANDARD as BASE64, Engine};
use color_eyre::eyre::{Result, WrapErr, eyre};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

use crate::dispatch::ScriptRegistry;
use crate::dispatch::activation::ActivationContext;
use crate::dispatch::script::ScriptContext;
use crate::fuse::VisibilityMap;
use crate::session::state;
use crate::types::ProcessVisibility;

/// Environment variable set inside the sandbox pointing to the control socket.
pub const NYNE_CONTROL_SOCKET_ENV: &str = "NYNE_CONTROL_SOCKET";

/// Inbound message types for the control socket.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    Exec {
        address: String,
        stdin: String,
    },
    Register {
        pid: i32,
        command: String,
    },
    Unregister {
        pid: i32,
    },
    SetVisibility {
        #[serde(default)]
        pid: Option<i32>,
        #[serde(default)]
        name: Option<String>,
        visibility: ProcessVisibility,
    },
    ListProcesses,
}

/// Outbound message types from the control socket.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Response {
    ExecOk { data: String },
    ExecErr { error: String },
    Registered,
    Unregistered,
    Visibility { rules: VisibilityRules },
    Processes { list: Vec<AttachedProcess> },
    Error { message: String },
}

/// A process currently attached to the sandbox via `nyne attach`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachedProcess {
    pub pid: i32,
    pub command: String,
    pub start_time: SystemTime,
}

/// Current visibility rule state returned by `SetVisibility` responses.
#[derive(Debug, Serialize, Deserialize)]
pub struct VisibilityRules {
    /// Per-PID explicit overrides (only registered processes).
    pub pid_rules: Vec<PidVisibility>,
    /// Dynamic name-based rules set at runtime.
    pub name_rules: Vec<NameVisibility>,
}

/// Visibility override for a specific PID.
#[derive(Debug, Serialize, Deserialize)]
pub struct PidVisibility {
    pub pid: i32,
    pub command: String,
    pub visibility: ProcessVisibility,
}

/// Visibility rule for a process comm name.
#[derive(Debug, Serialize, Deserialize)]
pub struct NameVisibility {
    pub name: String,
    pub visibility: ProcessVisibility,
}

/// Handle to a running control server. Joins the IPC thread and removes the socket on drop.
pub struct Server {
    socket_path: PathBuf,
    handle: Option<thread::JoinHandle<()>>,
}

/// Removes the Unix domain socket file on drop.
impl Drop for Server {
    /// Removes the socket file and joins the IPC thread for graceful shutdown.
    fn drop(&mut self) {
        if let Err(e) = fs::remove_file(&self.socket_path)
            && e.kind() != io::ErrorKind::NotFound
        {
            warn!(path = %self.socket_path.display(), error = %e, "failed to remove control socket");
        }
        if let Some(handle) = self.handle.take()
            && let Err(e) = handle.join()
        {
            warn!("control IPC thread panicked: {e:?}");
        }
    }
}

/// Shared state for tracking attached processes.
type ProcessTable = Arc<Mutex<Vec<AttachedProcess>>>;

/// Start the control server on the given socket path.
pub fn start_server(
    socket_path: &Path,
    registry: Arc<ScriptRegistry>,
    ctx: Arc<ActivationContext>,
    visibility: Arc<VisibilityMap>,
) -> Result<Server> {
    if socket_path.exists() {
        fs::remove_file(socket_path)
            .wrap_err_with(|| format!("removing stale control socket: {}", socket_path.display()))?;
    }

    let listener = UnixListener::bind(socket_path)
        .wrap_err_with(|| format!("binding control socket: {}", socket_path.display()))?;

    info!(path = %socket_path.display(), "control server listening");

    let processes: ProcessTable = Arc::new(Mutex::new(Vec::new()));
    let path_for_thread = socket_path.to_path_buf();

    let handle = thread::Builder::new()
        .name("control-ipc".into())
        .spawn(move || server_loop(&path_for_thread, &listener, &registry, &ctx, &processes, &visibility))
        .wrap_err("spawning control IPC thread")?;

    Ok(Server {
        socket_path: socket_path.to_path_buf(),
        handle: Some(handle),
    })
}

/// Accept connections on the Unix listener and dispatch each request.
fn server_loop(
    path: &Path,
    listener: &UnixListener,
    registry: &ScriptRegistry,
    activation: &ActivationContext,
    processes: &ProcessTable,
    visibility: &VisibilityMap,
) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) =>
                if let Err(e) = handle_connection(stream, registry, activation, processes, visibility) {
                    warn!(error = format!("{e:#}"), "control request failed");
                },
            Err(e) => {
                debug!(path = %path.display(), error = %e, "control listener stopped");
                break;
            }
        }
    }
}

/// Read a single JSON request from a stream, dispatch it, and write the response.
fn handle_connection(
    stream: UnixStream,
    registry: &ScriptRegistry,
    activation: &ActivationContext,
    processes: &ProcessTable,
    visibility: &VisibilityMap,
) -> Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line).wrap_err("reading request")?;

    let req: Request = match serde_json::from_str(&line) {
        Ok(req) => req,
        Err(e) => {
            let response = Response::Error {
                message: format!("{e:#}"),
            };
            let mut writer = stream;
            serde_json::to_writer(&mut writer, &response).wrap_err("writing error response")?;
            writer.write_all(b"\n")?;
            return Err(e).wrap_err("parsing request");
        }
    };
    let response = dispatch(req, registry, activation, processes, visibility);

    let mut writer = stream;
    serde_json::to_writer(&mut writer, &response).wrap_err("writing response")?;
    writer.write_all(b"\n")?;

    Ok(())
}

/// Route a control request to the appropriate handler.
fn dispatch(
    req: Request,
    registry: &ScriptRegistry,
    activation: &ActivationContext,
    processes: &ProcessTable,
    visibility: &VisibilityMap,
) -> Response {
    match req {
        Request::Exec { address, stdin } => handle_exec(&address, &stdin, registry, activation),
        Request::Register { pid, command } => handle_register(pid, command, processes),
        Request::Unregister { pid } => handle_unregister(pid, processes),
        Request::SetVisibility {
            pid,
            name,
            visibility: vis,
        } => handle_set_visibility(pid, name, vis, processes, visibility),
        Request::ListProcesses => handle_list(processes),
    }
}

/// Decode base64 stdin, execute the addressed script, and return the result.
fn handle_exec(address: &str, stdin_b64: &str, registry: &ScriptRegistry, activation: &ActivationContext) -> Response {
    let stdin = match BASE64.decode(stdin_b64) {
        Ok(bytes) => bytes,
        Err(e) => {
            return Response::Error {
                message: format!("decoding stdin: {e}"),
            };
        }
    };

    let payload = String::from_utf8_lossy(&stdin);
    trace!(target: "wire", address, %payload, "exec request");

    let ctx = ScriptContext::new(activation);
    match registry.exec(address, &ctx, &stdin) {
        Ok(stdout) => {
            let response = String::from_utf8_lossy(&stdout);
            trace!(target: "wire", address, %response, "exec response");
            Response::ExecOk {
                data: BASE64.encode(&stdout),
            }
        }
        Err(e) => {
            let msg = format!("{e:#}");
            error!(address, error = %msg, "script execution failed");
            Response::ExecErr { error: msg }
        }
    }
}

/// Register a process in the attached process table, replacing any prior entry for the same PID.
fn handle_register(pid: i32, command: String, processes: &ProcessTable) -> Response {
    let mut table = processes.lock();
    // Remove any existing entry for this PID (re-registration).
    table.retain(|p| p.pid != pid);
    info!(pid, %command, "client attached");
    table.push(AttachedProcess {
        pid,
        command,
        start_time: SystemTime::now(),
    });
    Response::Registered
}

/// Remove a process from the attached process table.
fn handle_unregister(pid: i32, processes: &ProcessTable) -> Response {
    processes.lock().retain(|p| p.pid != pid);
    info!(pid, "client detached");
    Response::Unregistered
}

/// Set or query visibility rules for a PID or process name.
fn handle_set_visibility(
    pid: Option<i32>,
    name: Option<String>,
    vis: ProcessVisibility,
    processes: &ProcessTable,
    map: &VisibilityMap,
) -> Response {
    match (pid, name) {
        (Some(_), Some(_)) => {
            return Response::Error {
                message: "specify either 'pid' or 'name', not both".into(),
            };
        }
        (Some(pid), None) => {
            if !processes.lock().iter().any(|p| p.pid == pid) {
                return Response::Error {
                    message: format!(
                        "PID {pid} is not a registered process (use ListProcesses to see registered PIDs)"
                    ),
                };
            }
            map.set_pid(pid.cast_unsigned(), vis);
            debug!(pid, %vis, "process visibility set");
        }
        (None, Some(name)) => {
            debug!(name = %name, %vis, "name visibility rule set");
            map.set_name_rule(name, vis);
        }
        (None, None) => {
            debug!("visibility rules queried");
        }
    }

    Response::Visibility {
        rules: build_visibility_rules(processes, map),
    }
}

/// Snapshot all active visibility rules (per-PID and per-name) into a response struct.
fn build_visibility_rules(processes: &ProcessTable, map: &VisibilityMap) -> VisibilityRules {
    let table = processes.lock();
    let pid_entries = map.explicit_pid_entries();

    let pid_rules = table
        .iter()
        .filter_map(|proc| {
            let vis = pid_entries.iter().find(|(pid, _)| *pid == proc.pid.cast_unsigned())?.1;
            Some(PidVisibility {
                pid: proc.pid,
                command: proc.command.clone(),
                visibility: vis,
            })
        })
        .collect();

    let name_rules = map
        .dynamic_name_rules()
        .into_iter()
        .map(|(name, visibility)| NameVisibility { name, visibility })
        .collect();

    VisibilityRules { pid_rules, name_rules }
}

/// Return all attached processes, pruning dead PIDs in the process.
fn handle_list(processes: &ProcessTable) -> Response {
    let mut table = processes.lock();
    // Prune dead PIDs while we're at it.
    table.retain(|p| state::is_pid_alive(p.pid));
    Response::Processes { list: table.clone() }
}

/// Send a control request and receive the response.
pub fn send_request(socket_path: &Path, req: &Request) -> Result<Response> {
    let mut stream = UnixStream::connect(socket_path)
        .wrap_err_with(|| format!("connecting to control socket: {}", socket_path.display()))?;

    serde_json::to_writer(&mut stream, req).wrap_err("writing request")?;
    stream.write_all(b"\n")?;
    stream.shutdown(Shutdown::Write)?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).wrap_err("reading response")?;

    serde_json::from_str(&line).wrap_err("parsing response")
}

/// Execute a script via the control socket. Returns stdout bytes.
pub fn exec_script(socket_path: &Path, address: &str, stdin: &[u8]) -> Result<Vec<u8>> {
    let req = Request::Exec {
        address: address.to_owned(),
        stdin: BASE64.encode(stdin),
    };

    match send_request(socket_path, &req)? {
        Response::ExecOk { data } => BASE64.decode(&data).wrap_err("decoding response data"),
        Response::ExecErr { error } => Err(eyre!("script error: {error}")),
        other => Err(eyre!("unexpected response to exec request: {other:?}")),
    }
}

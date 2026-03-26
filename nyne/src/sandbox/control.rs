//! Control socket — IPC between the daemon and CLI commands.
//!
//! Carries multiple message types over a single Unix domain socket:
//! - **Exec**: dispatch a provider script (used by `nyne exec`)
//! - **Register/Unregister**: track attached processes (used by `nyne attach`)
//! - **`ListProcesses`**: query attached processes (used by `nyne list`)
//!
//! Wire format: newline-delimited JSON, one request → one response per connection.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, thread};

use base64::prelude::{BASE64_STANDARD as BASE64, Engine};
use color_eyre::eyre::{WrapErr, eyre};
use parking_lot::Mutex;
use rustix::event::{PollFd, PollFlags, poll};
use rustix::pipe::{PipeFlags, pipe_with};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

use crate::dispatch::ScriptRegistry;
use crate::dispatch::script::ScriptContext;
use crate::fuse::VisibilityMap;
use crate::prelude::*;
use crate::session::state;
use crate::types::ProcessVisibility;

/// Environment variable set inside the sandbox pointing to the control socket.
///
/// Injected by `command_main` so that `nyne exec` (and other CLI commands
/// running inside the sandbox) can locate the daemon's Unix domain socket
/// for IPC without path discovery.
pub const NYNE_CONTROL_SOCKET_ENV: &str = "NYNE_CONTROL_SOCKET";

/// Inbound message types for the control socket.
///
/// This enum is the single source of truth for the control protocol.
/// The `nyne ctl` CLI reads a JSON `Request` directly rather than
/// maintaining a parallel type.
///
/// Note: `deny_unknown_fields` cannot be used here because the
/// `SetVisibility` variant uses `#[serde(flatten)]` for wire-format
/// compatibility, which is incompatible with `deny_unknown_fields`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    /// Execute a provider script by address (used by `nyne exec`).
    /// `stdin` is base64-encoded binary input.
    Exec { address: String, stdin: String },
    /// Register an attached process in the process table.
    Register { pid: i32, command: String },
    /// Remove an attached process from the process table.
    Unregister { pid: i32 },
    /// Set or query visibility level for a PID or process name.
    SetVisibility {
        #[serde(flatten)]
        target: VisibilityTarget,
        visibility: ProcessVisibility,
    },
    /// Query all attached processes (used by `nyne list`).
    ListProcesses,
}

/// Identifies the subject of a visibility change — either a specific PID or a
/// process name pattern.
///
/// Wire format (untagged): `{"pid": 123}` or `{"name": "fish"}` or `{}` (query-only).
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VisibilityTarget {
    /// Target a specific attached process by PID.
    Pid { pid: i32 },
    /// Target processes matching a comm name.
    Name { name: String },
    /// No target — query current rules without changing anything.
    Query {},
}

/// Outbound message types from the control socket.
///
/// Each variant corresponds to a specific [`Request`] type, except
/// [`Error`](Self::Error) which is a catch-all for protocol-level failures.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Response {
    /// Successful script execution. `data` is base64-encoded stdout.
    ExecOk { data: String },
    /// Script execution failed with the given error message.
    ExecErr { error: String },
    /// Process successfully registered.
    Registered,
    /// Process successfully unregistered.
    Unregistered,
    /// Current visibility rules snapshot (response to `SetVisibility`).
    Visibility { rules: VisibilityRules },
    /// List of all attached processes (response to `ListProcesses`).
    Processes { list: Vec<AttachedProcess> },
    /// Protocol-level error (malformed request, unknown type, etc.).
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
///
/// Shutdown uses a pipe: dropping the write end causes `POLLHUP` on the read
/// end inside the server loop's `poll()`, breaking the accept loop so the
/// thread exits and `join()` completes.
pub struct Server {
    socket_path: PathBuf,
    /// Write end of the shutdown pipe. Dropped to signal the server loop.
    shutdown_wr: Option<OwnedFd>,
    handle: Option<thread::JoinHandle<()>>,
}

/// Clean shutdown on drop: signal the server loop, join the IPC thread, and remove the socket.
///
/// Dropping the `shutdown_wr` pipe fd triggers `POLLHUP` in the server
/// loop, causing it to break out of its accept loop. The thread is then
/// joined to ensure all in-flight requests complete. Finally, the Unix
/// domain socket file is removed from disk (ignoring `NotFound` in case
/// it was already cleaned up).
impl Drop for Server {
    /// Signals the server loop to exit, joins the IPC thread, and removes the socket file.
    fn drop(&mut self) {
        // Close the write end of the shutdown pipe → POLLHUP on the read end
        // inside the server loop's poll(), breaking the accept loop.
        self.shutdown_wr.take();

        if let Some(handle) = self.handle.take()
            && let Err(e) = handle.join()
        {
            warn!("control IPC thread panicked: {e:?}");
        }

        if let Err(e) = fs::remove_file(&self.socket_path)
            && e.kind() != io::ErrorKind::NotFound
        {
            warn!(path = %self.socket_path.display(), error = %e, "failed to remove control socket");
        }
    }
}

/// Shared state for tracking attached processes.
///
/// A `Mutex<Vec<AttachedProcess>>` shared between the server loop thread
/// and request handlers. Entries are added on `Register`, removed on
/// `Unregister`, and pruned of dead PIDs on `ListProcesses`.
type ProcessTable = Arc<Mutex<Vec<AttachedProcess>>>;

/// Start the control server on the given socket path.
///
/// Removes any stale socket file, binds a new `UnixListener`, and spawns
/// the `control-ipc` thread running [`server_loop`]. Shutdown is coordinated
/// via a pipe — dropping the returned [`Server`] closes the write end,
/// which triggers `POLLHUP` in the server loop and causes it to exit.
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

    let (shutdown_rd, shutdown_wr) = pipe_with(PipeFlags::CLOEXEC).wrap_err("creating control server shutdown pipe")?;

    let processes: ProcessTable = Arc::new(Mutex::new(Vec::new()));
    let path_for_thread = socket_path.to_path_buf();

    let handle = thread::Builder::new()
        .name("control-ipc".into())
        .spawn(move || {
            server_loop(
                &path_for_thread,
                &listener,
                &shutdown_rd,
                &registry,
                &ctx,
                &processes,
                &visibility,
            );
        })
        .wrap_err("spawning control IPC thread")?;

    Ok(Server {
        socket_path: socket_path.to_path_buf(),
        shutdown_wr: Some(shutdown_wr),
        handle: Some(handle),
    })
}

/// Accept connections on the Unix listener and dispatch each request.
///
/// Blocks in `poll()` on both the listener and a shutdown pipe. When the
/// write end of the pipe is dropped (by [`Server::drop`]), `POLLHUP` wakes
/// the poll and the loop exits cleanly.
#[allow(clippy::too_many_arguments)]
fn server_loop(
    path: &Path,
    listener: &UnixListener,
    shutdown: &OwnedFd,
    registry: &ScriptRegistry,
    activation: &ActivationContext,
    processes: &ProcessTable,
    visibility: &VisibilityMap,
) {
    let listener_fd = listener.as_fd();
    loop {
        let mut fds = [
            PollFd::new(&listener_fd, PollFlags::IN),
            PollFd::new(&shutdown, PollFlags::IN),
        ];

        if poll(&mut fds, None).is_err() {
            break;
        }

        // Shutdown pipe closed → exit.
        if fds[1]
            .revents()
            .intersects(PollFlags::HUP | PollFlags::IN | PollFlags::ERR)
        {
            debug!(path = %path.display(), "control server shutting down");
            break;
        }

        // New connection ready.
        if !fds[0].revents().contains(PollFlags::IN) {
            continue;
        }
        match listener.accept() {
            Ok((stream, _addr)) =>
                if let Err(e) = handle_connection(&stream, registry, activation, processes, visibility) {
                    warn!(error = format!("{e:#}"), "control request failed");
                },
            Err(e) => {
                debug!(path = %path.display(), error = %e, "control listener stopped");
                break;
            }
        }
    }
}

/// Write a JSON response to the stream followed by a newline.
fn write_response(stream: &UnixStream, response: &Response) -> Result<()> {
    let mut writer = stream;
    serde_json::to_writer(&mut writer, response).wrap_err("writing response")?;
    writer.write_all(b"\n")?;
    Ok(())
}

/// Read a single JSON request from a stream, dispatch it, and write the response.
fn handle_connection(
    stream: &UnixStream,
    registry: &ScriptRegistry,
    activation: &ActivationContext,
    processes: &ProcessTable,
    visibility: &VisibilityMap,
) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).wrap_err("reading request")?;

    let req = match serde_json::from_str::<Request>(&line) {
        Ok(req) => req,
        Err(e) => {
            write_response(stream, &Response::Error {
                message: format!("{e:#}"),
            })
            .wrap_err("writing error response")?;
            return Err(e).wrap_err("parsing request");
        }
    };
    write_response(stream, &dispatch(req, registry, activation, processes, visibility))
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
            target,
            visibility: vis,
        } => handle_set_visibility(target, vis, processes, visibility),
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
///
/// Three target modes:
/// - `Pid { pid }` — set visibility for a specific registered PID (errors if
///   the PID is not in the process table)
/// - `Name { name }` — set a name-based rule matching process comm names
/// - `Query {}` — return current rules without modifying anything
///
/// Always returns the full visibility rules snapshot in the response.
fn handle_set_visibility(
    target: VisibilityTarget,
    vis: ProcessVisibility,
    processes: &ProcessTable,
    map: &VisibilityMap,
) -> Response {
    match target {
        VisibilityTarget::Pid { pid } => {
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
        VisibilityTarget::Name { name } => {
            debug!(name = %name, %vis, "name visibility rule set");
            map.set_name_rule(&name, vis);
        }
        VisibilityTarget::Query {} => {
            debug!("visibility rules queried");
        }
    }

    Response::Visibility {
        rules: build_visibility_rules(processes, map),
    }
}

/// Snapshot all active visibility rules (per-PID and per-name) into a response struct.
///
/// Joins the process table with the visibility map's explicit PID entries
/// to produce per-PID rules (only includes PIDs that have an explicit
/// override). Name-based rules are collected separately from the map's
/// dynamic name rules. The result is a complete snapshot suitable for
/// the `Visibility` response variant.
fn build_visibility_rules(processes: &ProcessTable, map: &VisibilityMap) -> VisibilityRules {
    let table = processes.lock();
    let pid_entries: HashMap<u32, _> = map.explicit_pid_entries().into_iter().collect();

    let pid_rules = table
        .iter()
        .filter_map(|proc| {
            let &vis = pid_entries.get(&proc.pid.cast_unsigned())?;
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
///
/// Opportunistically removes entries for PIDs that no longer exist
/// (checked via `/proc/<pid>` presence). This keeps the table clean
/// without requiring explicit unregister calls from crashed clients.
fn handle_list(processes: &ProcessTable) -> Response {
    let mut table = processes.lock();
    // Prune dead PIDs while we're at it.
    table.retain(|p| state::is_pid_alive(p.pid));
    Response::Processes { list: table.clone() }
}

/// Send a control request and receive the response.
///
/// Opens a new Unix stream connection to the control socket, writes the
/// request as newline-delimited JSON, shuts down the write half (signaling
/// end of request), and reads back a single JSON response line. Each
/// request uses a fresh connection — no persistent state between calls.
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
///
/// Convenience wrapper around [`send_request`] for the `Exec` request type.
/// Encodes `stdin` as base64, sends the request, and decodes the base64
/// response back to raw bytes. Used by `nyne exec` to dispatch provider
/// scripts through the daemon's script registry.
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

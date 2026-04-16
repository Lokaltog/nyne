//! Control socket — IPC between the daemon and CLI commands.
//!
//! Carries multiple message types over a single Unix domain socket:
//! - **Exec**: dispatch a provider script (used by `nyne exec`)
//! - **Register/Unregister**: track attached processes (used by `nyne attach`)
//! - **`ListProcesses`**: query attached processes (used by `nyne list`)
//!
//! Wire format: newline-delimited JSON, one request → one response per connection.

use std::io::{self, BufRead, BufReader, IoSlice, IoSliceMut, Write};
use std::mem::MaybeUninit;
use std::net::Shutdown;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::str::from_utf8;
use std::time::SystemTime;
use std::{fs, thread};

use base64::prelude::{BASE64_STANDARD as BASE64, Engine};
use color_eyre::eyre::{WrapErr, ensure, eyre};
use parking_lot::Mutex;
use rustix::event::{PollFd, PollFlags, poll};
use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags,
    recvmsg, sendmsg,
};
use rustix::pipe::{PipeFlags, pipe_with};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

use super::namespace::Namespace;
use crate::dispatch::script::ScriptContext;
use crate::dispatch::{ControlRegistry, ScriptRegistry};
use crate::plugin::control::{AttachedProcess, ControlContext, ProcessTable};
use crate::prelude::*;
use crate::process;
use crate::router::Chain;

/// Environment variable set inside the sandbox pointing to the control socket.
///
/// Injected by `command_main` so that `nyne exec` (and other CLI commands
/// running inside the sandbox) can locate the daemon's Unix domain socket
/// for IPC without path discovery.
pub const NYNE_CONTROL_SOCKET_ENV: &str = "NYNE_CONTROL_SOCKET";

/// Environment variable set inside the sandbox pointing to the session
/// directory where nested sessions live.
///
/// Injected by `command_main` alongside [`NYNE_CONTROL_SOCKET_ENV`] so
/// that `nyne mount`/`nyne list`/`nyne attach` invocations inside the
/// sandbox share a consistent session directory scoped to the parent
/// daemon, rather than each attach landing in its own namespace bucket.
pub const NYNE_SESSION_DIR_ENV: &str = "NYNE_SESSION_DIR";

/// Core control request types handled directly by the control server.
///
/// Plugin-provided commands are dispatched separately via the
/// [`ControlRegistry`](crate::dispatch::ControlRegistry) — any request
/// whose `type` field does not match a core variant is looked up there.
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
    /// Query all attached processes (used by `nyne list`).
    ListProcesses,
    /// Request the daemon's user and mount namespace fds for attach.
    ///
    /// The response carries the two fds as `SCM_RIGHTS` ancillary data
    /// (user first, mnt second) alongside a [`Response::NamespaceFds`]
    /// tag. This replaces resolving the daemon's PID via `SO_PEERCRED`
    /// and opening `/proc/<pid>/ns/*` — fd passing works cross-PID-ns,
    /// PID translation does not (sibling PID namespaces can't see each
    /// other).
    GetNamespaceFds,
}

/// Outbound message types from the control socket.
///
/// Core variants correspond to specific [`Request`] types. Plugin-provided
/// commands return [`Plugin`](Self::Plugin) with an opaque JSON payload.
/// [`Error`](Self::Error) is a catch-all for protocol-level failures.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Response {
    /// Successful script execution. `data` is base64-encoded stdout.
    ExecOk { data: String },
    /// Script execution failed with the given error message.
    ExecErr { error: String },
    /// Process successfully registered.
    Registered,
    /// Process successfully unregistered.
    Unregistered,
    /// List of all attached processes (response to `ListProcesses`).
    Processes { list: Vec<AttachedProcess> },
    /// Response to `GetNamespaceFds`. The actual fds travel as
    /// `SCM_RIGHTS` ancillary data on the same `recvmsg` call; this
    /// tag exists only for protocol sanity checking.
    NamespaceFds,
    /// Response from a plugin-provided control command.
    Plugin {
        #[serde(flatten)]
        payload: serde_json::Value,
    },
    /// Protocol-level error (malformed request, unknown type, etc.).
    Error { message: String },
}

/// Shared state used by the control server and every request handler.
///
/// Groups the per-daemon registries, process table, middleware chain,
/// and namespace fds so that the server loop and dispatch path don't
/// thread a half-dozen `Arc`s through every function signature.
pub struct Handlers {
    pub registry: Arc<ScriptRegistry>,
    pub activation: Arc<ActivationContext>,
    pub processes: ProcessTable,
    pub control_commands: Arc<ControlRegistry>,
    pub chain: Arc<Chain>,
    pub ns: Arc<Namespace>,
}

impl Handlers {
    /// Build a fresh set of handlers with an empty process table.
    pub fn new(
        registry: Arc<ScriptRegistry>,
        activation: Arc<ActivationContext>,
        control_commands: Arc<ControlRegistry>,
        chain: Arc<Chain>,
        ns: Arc<Namespace>,
    ) -> Self {
        Self {
            registry,
            activation,
            processes: Arc::new(Mutex::new(Vec::new())),
            control_commands,
            chain,
            ns,
        }
    }
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

/// Start the control server on the given socket path.
///
/// Removes any stale socket file, binds a new `UnixListener`, and spawns
/// the `control-ipc` thread running [`server_loop`]. Shutdown is coordinated
/// via a pipe — dropping the returned [`Server`] closes the write end,
/// which triggers `POLLHUP` in the server loop and causes it to exit.
pub fn start_server(socket_path: &Path, handlers: Handlers) -> Result<Server> {
    match fs::remove_file(socket_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).wrap_err_with(|| format!("removing stale control socket: {}", socket_path.display())),
    }

    let listener = UnixListener::bind(socket_path)
        .wrap_err_with(|| format!("binding control socket: {}", socket_path.display()))?;

    info!(path = %socket_path.display(), "control server listening");

    let (shutdown_rd, shutdown_wr) = pipe_with(PipeFlags::CLOEXEC).wrap_err("creating control server shutdown pipe")?;

    let path_for_thread = socket_path.to_path_buf();

    let handle = thread::Builder::new()
        .name("control-ipc".into())
        .spawn(move || {
            server_loop(&path_for_thread, &listener, &shutdown_rd, &handlers);
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
fn server_loop(path: &Path, listener: &UnixListener, shutdown: &OwnedFd, handlers: &Handlers) {
    let listener_fd = listener.as_fd();
    let mut fds = [
        PollFd::new(&listener_fd, PollFlags::IN),
        PollFd::new(&shutdown, PollFlags::IN),
    ];
    loop {
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
                if let Err(e) = handle_connection(&stream, handlers) {
                    warn!(error = format!("{e:#}"), "control request failed");
                },
            Err(e) => {
                debug!(path = %path.display(), error = %e, "control listener stopped");
                break;
            }
        }
    }
}

/// Serialize a response as a newline-terminated JSON line.
///
/// Single source of truth for the wire format — used both by the plain
/// [`write_response`] path and the ancillary-fd [`send_namespace_fds`]
/// path, which need the payload as a pre-built `Vec<u8>` for `sendmsg`.
fn response_line(response: &Response) -> Result<Vec<u8>> {
    let mut buf = serde_json::to_vec(response).wrap_err("serializing response")?;
    buf.push(b'\n');
    Ok(buf)
}

/// Write a JSON response to the stream followed by a newline.
fn write_response(stream: &UnixStream, response: &Response) -> Result<()> {
    (&mut &*stream)
        .write_all(&response_line(response)?)
        .wrap_err("writing response")
}

/// Read a single JSON request from a stream, dispatch it, and write the response.
///
/// Single-pass deserialization: parse as `Value` once, then try core
/// [`Request`] variants before falling back to plugin commands via the
/// [`ControlRegistry`].
fn handle_connection(stream: &UnixStream, handlers: &Handlers) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).wrap_err("reading request")?;

    let raw: serde_json::Value = serde_json::from_str(&line).map_err(|e| eyre!("{e:#}"))?;

    // Phase 1: try core request variants.
    if let Ok(req) = serde_json::from_value::<Request>(raw.clone()) {
        // `GetNamespaceFds` needs direct stream access to send fds via
        // SCM_RIGHTS ancillary, so it bypasses the `Response` return path.
        if matches!(req, Request::GetNamespaceFds) {
            return send_namespace_fds(stream, &handlers.ns);
        }
        return write_response(stream, &dispatch(req, handlers));
    }

    // Phase 2: extract `type` field and dispatch to plugin handlers.
    let Some(command_type) = raw.get("type").and_then(|t| t.as_str()).map(str::to_owned) else {
        return write_response(stream, &Response::Error {
            message: "missing \"type\" field".into(),
        });
    };
    let ctx = ControlContext {
        activation: &handlers.activation,
        processes: &handlers.processes,
    };
    write_response(
        stream,
        &match handlers.control_commands.dispatch(&command_type, raw, &ctx) {
            Some(payload) => Response::Plugin { payload },
            None => Response::Error {
                message: format!("unknown command: {command_type}"),
            },
        },
    )
}

/// Route a core control request to the appropriate handler.
fn dispatch(req: Request, handlers: &Handlers) -> Response {
    match req {
        Request::Exec { address, stdin } => handle_exec(
            &address,
            &stdin,
            &handlers.registry,
            &handlers.activation,
            &handlers.chain,
        ),
        Request::Register { pid, command } => handle_register(pid, command, &handlers.processes),
        Request::Unregister { pid } => handle_unregister(pid, &handlers.processes),
        Request::ListProcesses => handle_list(&handlers.processes),
        Request::GetNamespaceFds => unreachable!("GetNamespaceFds is intercepted in handle_connection"),
    }
}

/// Decode base64 stdin, execute the addressed script, and return the result.
fn handle_exec(
    address: &str,
    stdin_b64: &str,
    registry: &ScriptRegistry,
    activation: &ActivationContext,
    chain: &Chain,
) -> Response {
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

    let ctx = ScriptContext::new(activation, chain);
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

/// Return all attached processes, pruning dead PIDs in the process.
///
/// Opportunistically removes entries for PIDs that no longer exist
/// (checked via `/proc/<pid>` presence). This keeps the table clean
/// without requiring explicit unregister calls from crashed clients.
fn handle_list(processes: &ProcessTable) -> Response {
    let mut table = processes.lock();
    // Prune dead PIDs while we're at it.
    table.retain(|p| process::is_pid_alive(p.pid));
    Response::Processes { list: table.clone() }
}
/// Send the daemon's namespace fds over the control socket via `SCM_RIGHTS`.
///
/// Writes a `Response::NamespaceFds` JSON line as the normal payload,
/// with the two fds (user first, mnt second) attached as ancillary
/// data. The client receives both atomically via `recvmsg`.
fn send_namespace_fds(stream: &UnixStream, ns: &Namespace) -> Result<()> {
    let payload = response_line(&Response::NamespaceFds)?;

    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let fds = [ns.user.as_fd(), ns.mnt.as_fd()];
    ensure!(
        control.push(SendAncillaryMessage::ScmRights(&fds)),
        "pushing ScmRights onto ancillary buffer"
    );

    let iov = [IoSlice::new(&payload)];
    sendmsg(stream, &iov, &mut control, SendFlags::empty()).wrap_err("sendmsg for NamespaceFds")?;
    Ok(())
}

/// Connect to the control socket and send a JSON request.
///
/// Opens a fresh `UnixStream`, writes the request as newline-delimited
/// JSON, and shuts down the write half to signal end-of-request. Returns
/// the connected stream for the caller to read the response.
fn connect_and_send(socket_path: &Path, req: &impl Serialize) -> Result<UnixStream> {
    let mut stream = UnixStream::connect(socket_path)
        .wrap_err_with(|| format!("connecting to control socket: {}", socket_path.display()))?;

    serde_json::to_writer(&mut stream, req).wrap_err("writing request")?;
    stream.write_all(b"\n")?;
    stream.shutdown(Shutdown::Write)?;
    Ok(stream)
}

/// Send a control request and receive the response.
///
/// Opens a new Unix stream connection to the control socket, writes the
/// request as newline-delimited JSON, shuts down the write half (signaling
/// end of request), and reads back a single JSON response line. Each
/// request uses a fresh connection — no persistent state between calls.
///
/// Accepts any `Serialize` type — core [`Request`] variants and plugin
/// command payloads both work.
pub fn send_request(socket_path: &Path, req: &impl Serialize) -> Result<Response> {
    let stream = connect_and_send(socket_path, req)?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).wrap_err("reading response")?;

    serde_json::from_str(&line).wrap_err("parsing response")
}
/// Request the daemon's namespace fds and receive them via `SCM_RIGHTS`.
///
/// The daemon opens `/proc/self/ns/{user,mnt}` at startup and holds
/// the fds for the lifetime of the server. This function connects to
/// the control socket, sends [`Request::GetNamespaceFds`], and pulls
/// the two fds out of the ancillary buffer on the response.
///
/// The caller can `setns` directly from the returned [`Namespace`]
/// without needing to resolve the daemon's PID.
pub fn recv_namespace_fds(socket_path: &Path) -> Result<Namespace> {
    let stream = connect_and_send(socket_path, &Request::GetNamespaceFds)?;

    let mut iov_buf = [0u8; 256];
    let mut iov = [IoSliceMut::new(&mut iov_buf)];
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let received = recvmsg(&stream, &mut iov, &mut control, RecvFlags::empty()).wrap_err("recvmsg for NamespaceFds")?;

    let response_bytes = iov_buf.get(..received.bytes).ok_or_else(|| {
        eyre!(
            "recvmsg reported {} bytes, buffer only holds {}",
            received.bytes,
            iov_buf.len()
        )
    })?;
    let response_text = from_utf8(response_bytes).wrap_err("response not UTF-8")?;
    let response: Response = serde_json::from_str(response_text.trim_end()).wrap_err("parsing response")?;
    ensure!(
        matches!(response, Response::NamespaceFds),
        "expected NamespaceFds response, got {response:?}"
    );

    let mut fds: Vec<OwnedFd> = Vec::with_capacity(2);
    for msg in control.drain() {
        if let RecvAncillaryMessage::ScmRights(iter) = msg {
            fds.extend(iter);
        }
    }
    let [user, mnt] =
        <[OwnedFd; 2]>::try_from(fds).map_err(|fds| eyre!("expected 2 fds in ancillary, got {}", fds.len()))?;
    Ok(Namespace { user, mnt })
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

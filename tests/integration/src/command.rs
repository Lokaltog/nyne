//! Shell command output captured from `nyne attach` invocations.

use std::process::Output;

/// Captured stdout, stderr, and exit code from a command executed in the mount.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl CommandOutput {
    /// Whether the command succeeded (exit code 0).
    pub const fn is_ok(&self) -> bool { self.exit_code == 0 }
}

impl From<Output> for CommandOutput {
    fn from(output: Output) -> Self {
        Self {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
        }
    }
}

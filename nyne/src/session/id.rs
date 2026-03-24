//! Session identifier generation and validation.

use std::fmt;
use std::path::Path;

use color_eyre::eyre::{Result, ensure, eyre};

use super::SessionRegistry;
use crate::format::to_kebab_raw;

/// A validated session identifier.
///
/// Format: `[a-z0-9][a-z0-9-]*` — derived from the last path component
/// of the mount directory, or explicitly provided via `id:path` prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

/// Construction and validation for session identifiers.
impl SessionId {
    /// Create a `SessionId` from a pre-validated string.
    fn new(s: String) -> Result<Self> {
        ensure!(!s.is_empty(), "session ID cannot be empty");
        ensure!(
            s.as_bytes().first() != Some(&b'-'),
            "session ID must not start with a hyphen: {s:?}"
        );
        ensure!(
            s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "session ID must be lowercase alphanumeric with hyphens: {s:?}"
        );
        Ok(Self(s))
    }

    /// Derive a session ID from a mount path, deduplicating against active sessions.
    pub(crate) fn from_path(path: &Path, registry: &SessionRegistry) -> Result<Self> {
        let base = path
            .file_name()
            .ok_or_else(|| eyre!("mount path has no filename: {}", path.display()))?
            .to_string_lossy();

        let sanitized = sanitize(&base);
        ensure!(
            !sanitized.is_empty(),
            "mount path produces empty session ID: {}",
            path.display()
        );

        // Append -2, -3, ... if the base ID is already taken.
        let id = if registry.is_active(&sanitized) {
            (2..=u32::MAX)
                .map(|n| format!("{sanitized}-{n}"))
                .find(|candidate| !registry.is_active(candidate))
                .ok_or_else(|| eyre!("could not find an available session ID for {sanitized:?}"))?
        } else {
            sanitized
        };

        Self::new(id)
    }

    /// Create a session ID from an explicit user-provided prefix.
    ///
    /// Hard error if the ID is already active.
    pub(crate) fn from_explicit(id: &str, registry: &SessionRegistry) -> Result<Self> {
        let sanitized = sanitize(id);
        ensure!(
            !sanitized.is_empty(),
            "explicit session ID is empty after sanitization: {id:?}"
        );

        if registry.is_active(&sanitized) {
            return Err(eyre!(
                "session ID {sanitized:?} is already in use — stop the existing session first"
            ));
        }

        Self::new(sanitized)
    }

    pub(crate) fn as_str(&self) -> &str { &self.0 }
}

/// Displays the session ID as its inner string.
impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

/// Sanitize a string into a valid session ID component.
fn sanitize(s: &str) -> String { to_kebab_raw(s) }

#[cfg(test)]
mod tests;

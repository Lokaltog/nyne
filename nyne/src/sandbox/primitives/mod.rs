//! Linux syscall primitives for sandbox construction.
//!
//! Thin wrappers around mount(2), unshare(2), setns(2), fork(2), exec(2).
//! These modules carry zero sandbox-specific policy — just safe Rust APIs
//! around raw syscalls. Sandbox orchestration (daemon/attach flows, overlay
//! setup, state lifecycle) lives in the parent module.

pub mod mnt;
pub mod namespace;
pub mod process;

pub use namespace::Namespace;

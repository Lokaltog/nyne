//! Request-dispatch pipeline: chain, providers, requests, and route contexts.
//!
//! `Chain` threads a [`Request`] through successive [`Provider`]s until one
//! terminates. `Op` describes what the caller wants; `RouteCtx`/`OpGuard`
//! carry route-matching state alongside the request. Together they are the
//! request-processing machinery; node-model, tree construction, and storage
//! backends all build on top of this layer.

pub mod chain;
pub mod provider;
pub mod request;
pub mod route;

pub use chain::{Chain, Next};
pub use provider::{InvalidationEvent, Provider, ProviderId, ProviderMeta};
pub use request::{Op, Process, Request, StateSnapshot};
pub use route::{OpGuard, RouteCtx};

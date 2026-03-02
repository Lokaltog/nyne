//! `TypeId`-keyed heterogeneous map for typed property storage.

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// A `TypeId`-keyed heterogeneous map.
///
/// Stores at most one value per type. Used by [`VirtualNode`](crate::node::VirtualNode)
/// for provider-specific properties and by [`PipelineContext`](crate::dispatch::context::PipelineContext)
/// for middleware-to-middleware communication.
#[derive(Default)]
pub struct TypeMap {
    inner: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl TypeMap {
    /// Create an empty map.
    pub fn new() -> Self { Self::default() }

    /// Insert a typed value. Replaces any existing value of the same type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.inner.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Retrieve a reference to a typed value.
    pub fn get<T: 'static>(&self) -> Option<&T> { self.inner.get(&TypeId::of::<T>())?.downcast_ref() }
}

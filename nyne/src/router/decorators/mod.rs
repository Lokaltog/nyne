//! Capability decorators for wrapping [`Readable`] and [`Writable`] implementations.
//!
//! Decorators wrap inner capabilities to transform content on read or write.
//! Providers compose them by nesting: `SliceReadable(MyReadable)`.

mod slice;

pub use slice::{SliceReadable, SliceWritable, lazy_slice_node, slice_node};

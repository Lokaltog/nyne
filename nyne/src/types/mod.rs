//! Shared value types and context structs.

pub mod file_kind;
pub mod path_conventions;
pub mod real_fs;
pub mod slice;
pub mod type_map;
pub mod vfs_path;

mod ext_counts;
mod git_dir_name;
mod passthrough_processes;
mod process_visibility;
mod symbol_line_range;

pub use ext_counts::ExtensionCounts;
pub use file_kind::FileKind;
pub use git_dir_name::GitDirName;
pub use passthrough_processes::PassthroughProcesses;
pub use process_visibility::ProcessVisibility;
pub use real_fs::{OsFs, RealFs};
pub use symbol_line_range::{SymbolLineRange, line_of_byte};
pub use type_map::TypeMap;
pub use vfs_path::VfsPath;

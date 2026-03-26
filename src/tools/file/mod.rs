//! 工作区文件类工具：由单文件拆分为子模块，对外 API 与拆分前一致。
//!
//! 路径均为**相对于工作目录**的相对路径；`path` 子模块集中 `resolve_for_read` / `resolve_for_write` 等安全边界。

mod directory;
mod display_fmt;
mod extract;
mod inspect;
mod mutate;
mod path;
mod perm;
mod read_tool;
mod symlink;
mod tree_glob;
mod write_ops;

pub use directory::read_dir;
pub use extract::extract_in_file;
pub use inspect::{file_exists, hash_file, read_binary_meta};
pub use mutate::{append_file, create_dir, delete_dir, delete_file, search_replace};
pub(crate) use path::{canonical_workspace_root, resolve_for_read};
pub use perm::chmod_file;
pub use read_tool::read_file;
pub use symlink::symlink_info;
pub use tree_glob::{glob_files, list_tree};
pub use write_ops::{copy_file, create_file, modify_file, move_file};

#[cfg(test)]
mod tests;

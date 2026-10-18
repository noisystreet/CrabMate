//! 工作区文件类工具：由单文件拆分为子模块，对外 API 与拆分前一致。
//!
//! 路径均为**相对于工作目录**的相对路径；`path` 子模块集中 `resolve_for_read` / `resolve_for_write` 等安全边界。

mod commit_guard;
mod directory;
mod display_fmt;
mod edits_batch;
mod inspect;
mod mutate;
mod path;
mod perm;
mod read_file_pipeline;
mod read_tool;
mod replace_lines_stream;
mod revision;
mod tree_glob;
mod write_ops;
mod write_precheck;

pub use directory::read_dir;
pub use inspect::{hash_file, read_binary_meta};
pub use mutate::{
    append_file, append_file_try, create_dir, delete_dir, delete_files, search_replace,
    search_replace_try,
};
pub use path::{canonical_workspace_root, resolve_for_read, resolve_for_read_open};
pub use perm::chmod_file;
pub use read_tool::read_file;
pub use read_tool::read_file_try;
#[cfg(test)]
pub use read_tool::strip_read_file_output_header_for_tests;
pub use tree_glob::{glob_files, list_tree};
pub use write_ops::{
    copy_file, copy_file_try, create_file, create_file_try, modify_file, modify_file_try, move_file,
    move_file_try,
};

#[cfg(test)]
mod mutate_delete_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_guards_batch;
#[cfg(test)]
mod tests_metadata;
#[cfg(test)]
mod tests_revision;
#[cfg(test)]
mod tests_typed_write_ops;

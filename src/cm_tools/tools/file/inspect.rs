//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::cm_tools::tool_result::ToolError;

use super::path::{
    PathAccess, WorkspacePathCodes, path_for_tool_display, workspace_path_tool_error,
};

/// read_binary_meta：默认读取文件头参与哈希的字节数
const READ_BINARY_META_PREFIX_DEFAULT: usize = 8192;
/// read_binary_meta：前缀哈希最多读取字节（避免大文件读入过多）
const READ_BINARY_META_PREFIX_MAX: usize = 256 * 1024;

#[allow(clippy::result_large_err)]
fn required_json_path(v: &serde_json::Value) -> Result<String, ToolError> {
    v.get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| ToolError::invalid_args("错误：缺少 path 参数".to_string()))
}

#[allow(clippy::result_large_err)]
fn resolve_regular_file(
    paths: &PathAccess<'_>,
    path: &str,
    codes: WorkspacePathCodes,
    not_file_code: &'static str,
) -> Result<std::path::PathBuf, ToolError> {
    let target = paths.for_read(path)
        .map_err(|e| workspace_path_tool_error(e, codes))?;
    if !target.is_file() {
        return Err(ToolError::external_code(
            not_file_code,
            "错误：路径不是文件或不存在".to_string(),
        ));
    }
    Ok(target)
}

fn append_modified_unix(out: &mut String, meta: &std::fs::Metadata) {
    let modified_unix = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    if let Some(secs) = modified_unix {
        out.push_str(&format!("modified_unix_secs: {}\n", secs));
    } else {
        out.push_str("modified_unix_secs: (不可用)\n");
    }
}

#[allow(clippy::result_large_err)]
fn sha256_file_prefix_hex(target: &Path, to_read: usize) -> Result<String, ToolError> {
    let mut file =
        File::open(target).map_err(|e| ToolError::external_code("read_binary_meta_io_failed", format!("打开文件失败: {e}")))?;
    let mut buf = vec![0u8; to_read];
    if to_read > 0 {
        file.read_exact(&mut buf)
            .map_err(|e| ToolError::external_code("read_binary_meta_io_failed", format!("读取文件头失败: {e}")))?;
    }
    Ok(bytes_to_hex(&Sha256::digest(&buf)))
}

///
/// 参数：`path`（必填）；`prefix_hash_bytes`（可选，默认 8192，0 表示不算哈希，上限 256KiB）。
#[allow(clippy::result_large_err)]
pub fn read_binary_meta_try(args_json: &str, paths: &PathAccess<'_>) -> Result<String, ToolError> {
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let path = required_json_path(&v)?;

    let prefix_hash_bytes = v
        .get("prefix_hash_bytes")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(READ_BINARY_META_PREFIX_DEFAULT)
        .min(READ_BINARY_META_PREFIX_MAX);

    let target = resolve_regular_file(
        paths,
        &path,
        WorkspacePathCodes {
            outside: "read_binary_meta_workspace_resolve_failed",
            io: "read_binary_meta_io_failed",
        },
        "read_binary_meta_path_not_file",
    )?;

    let meta = std::fs::metadata(&target).map_err(|e| {
        ToolError::external_code("read_binary_meta_io_failed", format!("读取元数据失败: {e}"))
    })?;
    let size = meta.len();

    let mut out = String::new();
    out.push_str(&format!(
        "path: {}\n",
        path_for_tool_display(paths.root(), &target, Some(&path))
    ));
    out.push_str(&format!("size_bytes: {}\n", size));
    append_modified_unix(&mut out, &meta);

    if prefix_hash_bytes == 0 {
        out.push_str("sha256_prefix: (已跳过，prefix_hash_bytes=0)\n");
        out.push_str("sha256_prefix_bytes: 0\n");
        return Ok(out.trim_end().to_string());
    }

    let to_read = (size as usize).min(prefix_hash_bytes);
    let hex = sha256_file_prefix_hex(&target, to_read)?;
    out.push_str(&format!("sha256_prefix: {}\n", hex));
    out.push_str(&format!(
        "sha256_prefix_bytes: {}（文件共 {} 字节；仅头 {} 字节参与哈希）\n",
        to_read, size, to_read
    ));
    if (size as usize) > to_read {
        out.push_str("note: 文件大于前缀长度，哈希仅为文件头摘要，非整文件校验。\n");
    }
    Ok(out.trim_end().to_string())
}


fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

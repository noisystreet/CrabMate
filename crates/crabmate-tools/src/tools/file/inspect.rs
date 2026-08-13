//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use sha2::{Digest, Sha256, Sha512};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::path::{path_for_tool_display, resolve_for_read, tool_user_error_from_workspace_path};

/// read_binary_meta：默认读取文件头参与哈希的字节数
const READ_BINARY_META_PREFIX_DEFAULT: usize = 8192;
/// read_binary_meta：前缀哈希最多读取字节（避免大文件读入过多）
const READ_BINARY_META_PREFIX_MAX: usize = 256 * 1024;

/// hash_file：`max_bytes` 上限（仅哈希前缀时）
const HASH_FILE_MAX_PREFIX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// 流式读缓冲区
const HASH_FILE_BUF_SIZE: usize = 256 * 1024;
pub fn file_exists(args_json: &str, working_dir: &Path) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let path = match v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(p) => p,
        None => return "错误：缺少 path 参数".to_string(),
    };

    if path.starts_with('/') || path.contains("..") {
        return "错误：path 必须是工作区内相对路径，且不能包含 .. 或绝对路径".to_string();
    }

    let kind = v
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or("any")
        .trim()
        .to_lowercase();

    let target = working_dir.join(path);
    let exists = target.exists();
    let type_ok = match kind.as_str() {
        "file" => target.is_file(),
        "dir" => target.is_dir(),
        "any" => exists,
        _ => return "错误：kind 仅支持 file|dir|any".to_string(),
    };

    let mut out = String::new();
    out.push_str(&format!("path: {}\n", path));
    out.push_str(&format!("exists: {}\n", exists));
    out.push_str(&format!("type_match: {}\n", type_ok));
    out.push_str(&format!("kind: {}\n", kind));
    out.trim_end().to_string()
}

fn required_json_path(v: &serde_json::Value) -> Result<String, String> {
    v.get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| "错误：缺少 path 参数".to_string())
}

fn resolve_regular_file(working_dir: &Path, path: &str) -> Result<std::path::PathBuf, String> {
    let target = match resolve_for_read(working_dir, path) {
        Ok(p) => p,
        Err(e) => return Err(tool_user_error_from_workspace_path(e)),
    };
    if !target.is_file() {
        return Err("错误：路径不是文件或不存在".to_string());
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

fn sha256_file_prefix(target: &Path, to_read: usize) -> Result<String, String> {
    let mut file = File::open(target).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut buf = vec![0u8; to_read];
    if to_read > 0 {
        file.read_exact(&mut buf)
            .map_err(|e| format!("读取文件头失败: {}", e))?;
    }
    Ok(bytes_to_hex(&Sha256::digest(&buf)))
}
///
/// 参数：`path`（必填）；`prefix_hash_bytes`（可选，默认 8192，0 表示不算哈希，上限 256KiB）。
pub fn read_binary_meta(args_json: &str, working_dir: &Path) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let path = match required_json_path(&v) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let prefix_hash_bytes = v
        .get("prefix_hash_bytes")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(READ_BINARY_META_PREFIX_DEFAULT)
        .min(READ_BINARY_META_PREFIX_MAX);

    let target = match resolve_regular_file(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let meta = match std::fs::metadata(&target) {
        Ok(m) => m,
        Err(e) => return format!("读取元数据失败: {}", e),
    };
    let size = meta.len();

    let mut out = String::new();
    out.push_str(&format!(
        "path: {}\n",
        path_for_tool_display(working_dir, &target, Some(&path))
    ));
    out.push_str(&format!("size_bytes: {}\n", size));
    append_modified_unix(&mut out, &meta);

    if prefix_hash_bytes == 0 {
        out.push_str("sha256_prefix: (已跳过，prefix_hash_bytes=0)\n");
        out.push_str("sha256_prefix_bytes: 0\n");
        return out.trim_end().to_string();
    }

    let to_read = (size as usize).min(prefix_hash_bytes);
    let hex = match sha256_file_prefix(&target, to_read) {
        Ok(h) => h,
        Err(e) => return e,
    };
    out.push_str(&format!("sha256_prefix: {}\n", hex));
    out.push_str(&format!(
        "sha256_prefix_bytes: {}（文件共 {} 字节；仅头 {} 字节参与哈希）\n",
        to_read, size, to_read
    ));
    if (size as usize) > to_read {
        out.push_str("note: 文件大于前缀长度，哈希仅为文件头摘要，非整文件校验。\n");
    }
    out.trim_end().to_string()
}

/// 计算工作区内**常规文件**的加密哈希（只读，流式读取，不把整文件载入内存）。
///
/// 参数：`path`（必填）；`algorithm`：`sha256`（默认）、`sha512`；`max_bytes` 可选，若设置则只哈希文件前若干字节（上限 4GiB），省略则整文件。
pub fn hash_file(args_json: &str, working_dir: &Path) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let path = match required_json_path(&v) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let algo = v
        .get("algorithm")
        .and_then(|a| a.as_str())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "sha256".to_string());

    let max_bytes = match parse_hash_max_bytes(&v) {
        Ok(m) => m,
        Err(e) => return e,
    };

    let target = match resolve_regular_file(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let meta = match std::fs::metadata(&target) {
        Ok(m) => m,
        Err(e) => return format!("读取元数据失败: {}", e),
    };
    let size = meta.len();
    let limit = max_bytes.map(|m| m.min(size)).unwrap_or(size);

    match hash_digest_for_algo(&algo, &target, limit) {
        Ok(hex_digest) => format_hash_file_ok(HashFileOkFmt {
            working_dir,
            target: &target,
            path: &path,
            size,
            limit,
            algo: &algo,
            hex_digest: &hex_digest,
            prefix_only: max_bytes.is_some() && limit < size,
        }),
        Err(e) => e,
    }
}

fn parse_hash_max_bytes(v: &serde_json::Value) -> Result<Option<u64>, String> {
    match v.get("max_bytes").and_then(|n| n.as_u64()) {
        Some(0) => Err("错误：max_bytes 须大于 0；省略该字段表示哈希整文件".to_string()),
        Some(n) => Ok(Some(n.min(HASH_FILE_MAX_PREFIX_BYTES))),
        None => Ok(None),
    }
}

fn hash_digest_for_algo(algo: &str, target: &Path, limit: u64) -> Result<String, String> {
    match algo {
        "sha256" | "sha-256" => hash_file_stream_sha256(target, limit),
        "sha512" | "sha-512" => hash_file_stream_sha512(target, limit),
        _ => Err(format!(
            "错误：algorithm 仅支持 sha256、sha512（收到 {:?}）",
            algo
        )),
    }
}

struct HashFileOkFmt<'a> {
    working_dir: &'a Path,
    target: &'a Path,
    path: &'a str,
    size: u64,
    limit: u64,
    algo: &'a str,
    hex_digest: &'a str,
    prefix_only: bool,
}

fn format_hash_file_ok(f: HashFileOkFmt<'_>) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "path: {}\n",
        path_for_tool_display(f.working_dir, f.target, Some(f.path))
    ));
    out.push_str(&format!("size_bytes: {}\n", f.size));
    out.push_str(&format!("hashed_bytes: {}\n", f.limit));
    out.push_str(&format!("algorithm: {}\n", f.algo));
    out.push_str(&format!("digest_hex: {}\n", f.hex_digest));
    if f.prefix_only {
        out.push_str("note: 仅前 hashed_bytes 参与哈希，非整文件。\n");
    }
    out.trim_end().to_string()
}

fn hash_file_stream_sha256(path: &Path, max_read: u64) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_FILE_BUF_SIZE];
    let mut remaining = max_read;
    while remaining > 0 {
        let chunk = (remaining as usize).min(buf.len());
        let n = file
            .read(&mut buf[..chunk])
            .map_err(|e| format!("读取文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        remaining -= n as u64;
    }
    Ok(bytes_to_hex(&hasher.finalize()))
}

fn hash_file_stream_sha512(path: &Path, max_read: u64) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut hasher = Sha512::new();
    let mut buf = vec![0u8; HASH_FILE_BUF_SIZE];
    let mut remaining = max_read;
    while remaining > 0 {
        let chunk = (remaining as usize).min(buf.len());
        let n = file
            .read(&mut buf[..chunk])
            .map_err(|e| format!("读取文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        remaining -= n as u64;
    }
    Ok(bytes_to_hex(&hasher.finalize()))
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

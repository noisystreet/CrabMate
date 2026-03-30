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
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
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

/// 只读二进制/任意文件的**元数据**：大小、可选修改时间、文件头一段的 SHA256（不把整文件载入上下文）。
///
/// 参数：`path`（必填）；`prefix_hash_bytes`（可选，默认 8192，0 表示不算哈希，上限 256KiB）。
pub fn read_binary_meta(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(p) => p.to_string(),
        None => return "错误：缺少 path 参数".to_string(),
    };

    let prefix_hash_bytes = v
        .get("prefix_hash_bytes")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(READ_BINARY_META_PREFIX_DEFAULT)
        .min(READ_BINARY_META_PREFIX_MAX);

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在".to_string();
    }

    let meta = match std::fs::metadata(&target) {
        Ok(m) => m,
        Err(e) => return format!("读取元数据失败: {}", e),
    };
    let size = meta.len();
    let modified_unix = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let mut out = String::new();
    out.push_str(&format!(
        "path: {}\n",
        path_for_tool_display(working_dir, &target, Some(&path))
    ));
    out.push_str(&format!("size_bytes: {}\n", size));

    if let Some(secs) = modified_unix {
        out.push_str(&format!("modified_unix_secs: {}\n", secs));
    } else {
        out.push_str("modified_unix_secs: (不可用)\n");
    }

    if prefix_hash_bytes == 0 {
        out.push_str("sha256_prefix: (已跳过，prefix_hash_bytes=0)\n");
        out.push_str("sha256_prefix_bytes: 0\n");
        return out.trim_end().to_string();
    }

    let to_read = (size as usize).min(prefix_hash_bytes);
    let mut file = match File::open(&target) {
        Ok(f) => f,
        Err(e) => return format!("打开文件失败: {}", e),
    };
    let mut buf = vec![0u8; to_read];
    if to_read > 0
        && let Err(e) = file.read_exact(&mut buf)
    {
        return format!("读取文件头失败: {}", e);
    }

    let digest = Sha256::digest(&buf);
    let hex = bytes_to_hex(&digest);
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
/// 参数：`path`（必填）；`algorithm`：`sha256`（默认）、`blake3`、`sha512`；`max_bytes` 可选，若设置则只哈希文件前若干字节（上限 4GiB），省略则整文件。
pub fn hash_file(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(p) => p.to_string(),
        None => return "错误：缺少 path 参数".to_string(),
    };

    let algo = v
        .get("algorithm")
        .and_then(|a| a.as_str())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "sha256".to_string());

    let max_bytes = match v.get("max_bytes").and_then(|n| n.as_u64()) {
        Some(0) => return "错误：max_bytes 须大于 0；省略该字段表示哈希整文件".to_string(),
        Some(n) => Some(n.min(HASH_FILE_MAX_PREFIX_BYTES)),
        None => None,
    };

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在".to_string();
    }

    let meta = match std::fs::metadata(&target) {
        Ok(m) => m,
        Err(e) => return format!("读取元数据失败: {}", e),
    };
    let size = meta.len();

    let limit = max_bytes.map(|m| m.min(size)).unwrap_or(size);

    let hash_result = match algo.as_str() {
        "sha256" | "sha-256" => hash_file_stream_sha256(&target, limit),
        "sha512" | "sha-512" => hash_file_stream_sha512(&target, limit),
        "blake3" => hash_file_stream_blake3(&target, limit),
        _ => {
            return format!(
                "错误：algorithm 仅支持 sha256、sha512、blake3（收到 {:?}）",
                algo
            );
        }
    };

    match hash_result {
        Ok(hex_digest) => {
            let mut out = String::new();
            out.push_str(&format!(
                "path: {}\n",
                path_for_tool_display(working_dir, &target, Some(&path))
            ));
            out.push_str(&format!("size_bytes: {}\n", size));
            out.push_str(&format!("hashed_bytes: {}\n", limit));
            out.push_str(&format!("algorithm: {}\n", algo));
            out.push_str(&format!("digest_hex: {}\n", hex_digest));
            if max_bytes.is_some() && limit < size {
                out.push_str("note: 仅前 hashed_bytes 参与哈希，非整文件。\n");
            }
            out.trim_end().to_string()
        }
        Err(e) => e,
    }
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

fn hash_file_stream_blake3(path: &Path, max_read: u64) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut hasher = blake3::Hasher::new();
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
    Ok(hasher.finalize().to_hex().to_string())
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

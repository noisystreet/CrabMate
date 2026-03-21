//! Rust 编译器 JSON 诊断（`cargo --message-format=json`）与可选的 **rust-analyzer** LSP（stdio）。
//!
//! rust-analyzer 需已安装并在 PATH 中（或由 `server_path` 指定）。LSP 为最小实现：initialize → didOpen → 单次请求。

use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct KillRaChild(Option<Child>);
impl Drop for KillRaChild {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

const MAX_DID_OPEN_BYTES: usize = 512 * 1024;
const DEFAULT_MAX_DIAGNOSTICS: usize = 120;
const DEFAULT_RA_WAIT_MS: u64 = 500;
const LSP_IO_TIMEOUT: Duration = Duration::from_secs(25);

// ---------- cargo / rustc JSON（compiler-message）----------

/// 运行 `cargo check --message-format=json`，解析 `compiler-message` 行并汇总为可读文本（不整段原始 JSON 灌给模型）。
pub fn rust_compiler_json(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：工作区根目录未找到 Cargo.toml".to_string();
    }

    let all_targets = v
        .get("all_targets")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let package = v
        .get("package")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let features = v
        .get("features")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let all_features = v
        .get("all_features")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let max_diag = v
        .get("max_diagnostics")
        .and_then(|x| x.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(DEFAULT_MAX_DIAGNOSTICS)
        .min(500);
    let format_kind = v
        .get("message_format")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("json");

    let mut cmd = Command::new("cargo");
    cmd.arg("check")
        .arg("--message-format")
        .arg(format_kind)
        .current_dir(workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if all_targets {
        cmd.arg("--all-targets");
    }
    if let Some(p) = package {
        cmd.arg("--package").arg(p);
    }
    if let Some(f) = features {
        cmd.arg("--features").arg(f);
    }
    if all_features {
        cmd.arg("--all-features");
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return format!("无法执行 cargo check: {}", e),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let exit = output.status.code().unwrap_or(-1);

    let mut diags: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if msg.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let Some(inner) = msg.get("message") else {
            continue;
        };
        if diags.len() >= max_diag {
            break;
        }
        diags.push(format_compiler_message(inner));
    }

    let mut out = String::new();
    out.push_str(&format!(
        "rust_compiler_json: cargo check --message-format={} (exit={})\n",
        format_kind, exit
    ));
    if !stderr.trim().is_empty() {
        out.push_str("--- cargo/stderr ---\n");
        out.push_str(&truncate_str(stderr.trim(), max_output_len.min(8192)));
        out.push_str("\n---\n");
    }
    if diags.is_empty() {
        out.push_str("（未解析到 compiler-message 行；若仅有 stderr 请见上文。可能编译通过或无 JSON 行。）\n");
    } else {
        out.push_str(&format!(
            "共 {} 条诊断（最多展示 {} 条）：\n\n",
            diags.len(),
            max_diag
        ));
        for (i, d) in diags.iter().enumerate() {
            out.push_str(&format!("--- [{}] ---\n{}\n", i + 1, d));
        }
    }

    truncate_str(&out, max_output_len).to_string()
}

fn format_compiler_message(m: &Value) -> String {
    let level = m.get("level").and_then(|x| x.as_str()).unwrap_or("?");
    let code = m
        .get("code")
        .and_then(|c| c.get("code").and_then(|x| x.as_str()))
        .unwrap_or("");
    let text = m.get("message").and_then(|x| x.as_str()).unwrap_or("");
    let mut s = format!("[{}]", level);
    if !code.is_empty() {
        s.push_str(&format!(" {}", code));
    }
    s.push_str(&format!(" {}\n", text));

    if let Some(rendered) = m.get("rendered").and_then(|x| x.as_str())
        && !rendered.trim().is_empty()
    {
        s.push_str(rendered.trim_end());
        s.push('\n');
    }

    if let Some(spans) = m.get("spans").and_then(|x| x.as_array()) {
        for sp in spans.iter().take(5) {
            let file = sp.get("file_name").and_then(|x| x.as_str()).unwrap_or("?");
            let line = sp.get("line_start").and_then(|x| x.as_u64()).unwrap_or(0);
            let col = sp.get("column_start").and_then(|x| x.as_u64()).unwrap_or(0);
            let is_primary = sp
                .get("is_primary")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let label = sp.get("label").and_then(|x| x.as_str()).unwrap_or("");
            let p = if is_primary { " (primary)" } else { "" };
            s.push_str(&format!(
                "  {}{}:{}:{}{} {}\n",
                file,
                p,
                line,
                col,
                if label.is_empty() { "" } else { " — " },
                label
            ));
        }
        if spans.len() > 5 {
            s.push_str(&format!("  … 另有 {} 个 span\n", spans.len() - 5));
        }
    }

    if let Some(children) = m.get("children").and_then(|x| x.as_array()) {
        for ch in children.iter().take(8) {
            s.push_str(&format_compiler_message(ch));
        }
        if children.len() > 8 {
            s.push_str(&format!("… 另有 {} 条子诊断\n", children.len() - 8));
        }
    }
    s.trim_end().to_string()
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}\n\n... (已截断，共 {} 字节)", &s[..max], s.len())
    }
}

// ---------- rust-analyzer LSP（stdio）----------

/// `textDocument/definition`：path 相对工作区，**line / character 为 0-based**（与 LSP 一致）。
pub fn rust_analyzer_goto_definition(args_json: &str, workspace_root: &Path) -> String {
    match lsp_definition_or_references(args_json, workspace_root, true) {
        Ok(s) => s,
        Err(e) => e,
    }
}

/// `textDocument/references`（`include_declaration` 默认 true）。
pub fn rust_analyzer_find_references(args_json: &str, workspace_root: &Path) -> String {
    match lsp_definition_or_references(args_json, workspace_root, false) {
        Ok(s) => s,
        Err(e) => e,
    }
}

fn lsp_definition_or_references(
    args_json: &str,
    workspace_root: &Path,
    definition: bool,
) -> Result<String, String> {
    let v: Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return Err(format!("参数 JSON 无效: {}", e)),
    };
    let path_rel = v
        .get("path")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "缺少 path".to_string())?;
    let line = v
        .get("line")
        .and_then(|x| x.as_u64())
        .ok_or("缺少 line（0-based）")? as u32;
    let character = v.get("character").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let server_path = v
        .get("server_path")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("rust-analyzer");
    let wait_ms = v
        .get("wait_after_open_ms")
        .and_then(|x| x.as_u64())
        .unwrap_or(DEFAULT_RA_WAIT_MS)
        .min(5000);

    let root = workspace_root
        .canonicalize()
        .map_err(|e| format!("工作区路径无法解析: {}", e))?;
    let file_path = resolve_rel_file(&root, path_rel)?;
    let uri = path_to_file_uri(&file_path)?;
    let text = fs::read_to_string(&file_path).map_err(|e| format!("读取源文件失败: {}", e))?;
    if text.len() > MAX_DID_OPEN_BYTES {
        return Err(format!(
            "文件过大 (>{})，请用 read_file 分段或缩小文件",
            MAX_DID_OPEN_BYTES
        ));
    }

    let root_uri = path_to_file_uri(&root)?;

    let child = Command::new(server_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            format!(
                "无法启动 rust-analyzer（{}）：{}。请安装 rust-analyzer 或设置 server_path。",
                server_path, e
            )
        })?;
    let mut guard = KillRaChild(Some(child));
    let c = guard.0.as_mut().ok_or("internal")?;

    let mut stdin = c.stdin.take().ok_or("stdin")?;
    let stdout = c.stdout.take().ok_or("stdout")?;
    let mut reader = BufReader::new(stdout);
    let deadline = Instant::now() + LSP_IO_TIMEOUT;

    let init = json!({
        "jsonrpc": "2.0",
        "id": 1u64,
        "method": "initialize",
        "params": {
            "processId": std::process::id(),
            "rootPath": root.to_string_lossy(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "publishDiagnostics": {},
                    "definition": { "linkSupport": false }
                },
                "workspace": {}
            },
            "workspaceFolders": [{
                "uri": root_uri,
                "name": "workspace"
            }]
        }
    });
    write_lsp(&mut stdin, &init.to_string()).map_err(|e| e.to_string())?;
    let _ = read_response_until_id(&mut reader, 1, deadline)?;

    let notif_init = json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    write_lsp(&mut stdin, &notif_init.to_string()).map_err(|e| e.to_string())?;

    let did_open = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "rust",
                "version": 1,
                "text": text
            }
        }
    });
    write_lsp(&mut stdin, &did_open.to_string()).map_err(|e| e.to_string())?;

    std::thread::sleep(Duration::from_millis(wait_ms));

    let req_id = 2u64;
    let params = if definition {
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        })
    } else {
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": v.get("include_declaration").and_then(|x| x.as_bool()).unwrap_or(true) }
        })
    };
    let method = if definition {
        "textDocument/definition"
    } else {
        "textDocument/references"
    };
    let req = json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params
    });
    write_lsp(&mut stdin, &req.to_string()).map_err(|e| e.to_string())?;

    let resp = read_response_until_id(&mut reader, req_id, deadline)?;
    drop(stdin);
    format_lsp_locations(&resp, definition)
}

fn format_lsp_locations(resp: &Value, definition: bool) -> Result<String, String> {
    let title = if definition {
        "rust_analyzer goto definition"
    } else {
        "rust_analyzer find references"
    };

    if let Some(err) = resp.get("error") {
        return Err(format!("{}: LSP error: {}", title, err));
    }

    let result = resp.get("result");
    let Some(result) = result else {
        return Ok(format!("{}: (空结果)", title));
    };

    if result.is_null() {
        return Ok(format!("{}: (无目标)", title));
    }

    let mut lines = vec![format!("{}:", title)];
    if let Some(arr) = result.as_array() {
        for loc in arr {
            lines.push(format_one_location(loc));
        }
    } else {
        lines.push(format_one_location(result));
    }
    Ok(lines.join("\n"))
}

fn format_one_location(v: &Value) -> String {
    if let Some(target_uri) = v.get("uri").and_then(|x| x.as_str()) {
        let range = v.get("range");
        let pos = range
            .and_then(|r| r.get("start"))
            .map(|s| {
                let l = s.get("line").and_then(|x| x.as_u64()).unwrap_or(0);
                let c = s.get("character").and_then(|x| x.as_u64()).unwrap_or(0);
                format!("{}:{}", l + 1, c + 1)
            })
            .unwrap_or_else(|| "?".to_string());
        format!("  {} @ {}", uri_to_brief_path(target_uri), pos)
    } else if let Some(target) = v.get("targetUri").and_then(|x| x.as_str()) {
        let range = v
            .get("targetRange")
            .or_else(|| v.get("targetSelectionRange"));
        let pos = range
            .and_then(|r| r.get("start"))
            .map(|s| {
                let l = s.get("line").and_then(|x| x.as_u64()).unwrap_or(0);
                let c = s.get("character").and_then(|x| x.as_u64()).unwrap_or(0);
                format!("{}:{}", l + 1, c + 1)
            })
            .unwrap_or_else(|| "?".to_string());
        format!("  {} @ {} (link)", uri_to_brief_path(target), pos)
    } else {
        format!("  {}", v.to_string().chars().take(200).collect::<String>())
    }
}

fn uri_to_brief_path(uri: &str) -> String {
    if let Some(rest) = uri.strip_prefix("file://") {
        rest.to_string()
    } else {
        uri.to_string()
    }
}

fn resolve_rel_file(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if Path::new(rel).is_absolute() || rel.contains("..") {
        return Err("path 必须为工作区内相对路径且不含 ..".to_string());
    }
    let joined = root.join(rel);
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))?;
    if !canon.starts_with(root) {
        return Err("路径超出工作区".to_string());
    }
    if !canon.is_file() {
        return Err("不是已存在的文件".to_string());
    }
    Ok(canon)
}

fn path_to_file_uri(p: &Path) -> Result<String, String> {
    let canon = p
        .canonicalize()
        .map_err(|e| format!("canonicalize: {}", e))?;
    let raw = canon.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        let t = raw.trim_start_matches(r"\\?\").replace('\\', "/");
        Ok(format!("file:///{}", t))
    }
    #[cfg(not(windows))]
    {
        Ok(format!("file://{}", raw))
    }
}

fn write_lsp(w: &mut impl Write, body: &str) -> std::io::Result<()> {
    let b = body.as_bytes();
    write!(w, "Content-Length: {}\r\n\r\n", b.len())?;
    w.write_all(b)?;
    w.flush()?;
    Ok(())
}

fn read_response_until_id<R: BufRead>(
    reader: &mut R,
    expected_id: u64,
    deadline: Instant,
) -> Result<Value, String> {
    loop {
        if Instant::now() > deadline {
            return Err("LSP 读取超时".to_string());
        }
        let buf = read_one_lsp_message(reader).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_slice(&buf).map_err(|e| format!("JSON: {}", e))?;
        if v.get("id").and_then(|x| x.as_u64()) == Some(expected_id) {
            return Ok(v);
        }
    }
}

fn read_one_lsp_message<R: BufRead>(reader: &mut R) -> Result<Vec<u8>, String> {
    let mut content_len: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("读 LSP 头: {}", e))?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_len = Some(
                rest.trim()
                    .parse::<usize>()
                    .map_err(|_| "Content-Length 无效".to_string())?,
            );
        }
    }
    let len = content_len.ok_or_else(|| "缺少 Content-Length".to_string())?;
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("读 LSP 体: {}", e))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_compiler_message_smoke() {
        let m = json!({
            "level": "error",
            "message": "oops",
            "code": { "code": "E0000" },
            "spans": [{
                "file_name": "src/lib.rs",
                "line_start": 1,
                "column_start": 0,
                "is_primary": true,
                "label": "here"
            }]
        });
        let s = format_compiler_message(&m);
        assert!(s.contains("error"));
        assert!(s.contains("E0000"));
        assert!(s.contains("src/lib.rs"));
    }
}

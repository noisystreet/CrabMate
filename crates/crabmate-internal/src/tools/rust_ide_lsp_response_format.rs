//! rust-analyzer LSP 响应格式化为可读文本（从 **`rust_ide.rs`** 拆出以降低单文件物理行数、满足 `fn-nloc` 棘轮）。

use serde_json::Value;

pub(super) fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}\n\n... (已截断，共 {} 字节)", &s[..max], s.len())
    }
}

pub(super) fn format_lsp_locations(resp: &Value, title: &str) -> Result<String, String> {
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

pub(super) fn format_lsp_document_highlights(resp: &Value) -> Result<String, String> {
    const TITLE: &str = "rust_analyzer document_highlight";
    if let Some(err) = resp.get("error") {
        return Err(format!("{TITLE}: LSP error: {err}"));
    }
    let Some(result) = resp.get("result") else {
        return Ok(format!("{TITLE}: (空结果)"));
    };
    if result.is_null() {
        return Ok(format!("{TITLE}: (无高亮)"));
    }
    let Some(arr) = result.as_array() else {
        return Ok(format!(
            "{TITLE}: (非数组: {})",
            truncate_str(&result.to_string(), 160)
        ));
    };
    if arr.is_empty() {
        return Ok(format!("{TITLE}: (无高亮)"));
    }
    let mut lines = vec![format!("{TITLE}:")];
    for (i, h) in arr.iter().enumerate().take(64) {
        let kind = h
            .get("kind")
            .and_then(|x| x.as_u64())
            .map(document_highlight_kind_name)
            .unwrap_or("?");
        let range = h.get("range");
        let span = range
            .and_then(range_to_1based_span)
            .unwrap_or_else(|| "?".to_string());
        lines.push(format!("  [{}] {} {}", i + 1, kind, span));
    }
    if arr.len() > 64 {
        lines.push(format!("  … 另有 {} 条", arr.len() - 64));
    }
    Ok(lines.join("\n"))
}

fn document_highlight_kind_name(k: u64) -> &'static str {
    match k {
        1 => "Text",
        2 => "Read",
        3 => "Write",
        _ => "Unknown",
    }
}

fn range_to_1based_span(r: &Value) -> Option<String> {
    let s = r.get("start")?;
    let e = r.get("end")?;
    let l0 = s.get("line").and_then(|x| x.as_u64()).unwrap_or(0) + 1;
    let c0 = s.get("character").and_then(|x| x.as_u64()).unwrap_or(0) + 1;
    let l1 = e.get("line").and_then(|x| x.as_u64()).unwrap_or(0) + 1;
    let c1 = e.get("character").and_then(|x| x.as_u64()).unwrap_or(0) + 1;
    Some(format!("{l0}:{c0}–{l1}:{c1}"))
}

pub(super) fn format_workspace_symbols(resp: &Value, max: usize) -> Result<String, String> {
    const TITLE: &str = "rust_analyzer workspace_symbol";
    if let Some(err) = resp.get("error") {
        return Err(format!("{TITLE}: LSP error: {err}"));
    }
    let Some(result) = resp.get("result") else {
        return Ok(format!("{TITLE}: (空结果)"));
    };
    if result.is_null() {
        return Ok(format!("{TITLE}: (无匹配)"));
    }
    let Some(arr) = result.as_array() else {
        return Ok(format!(
            "{TITLE}: (非数组: {})",
            truncate_str(&result.to_string(), 160)
        ));
    };
    if arr.is_empty() {
        return Ok(format!("{TITLE}: (无匹配)"));
    }
    let mut lines = vec![format!("{TITLE}:")];
    for s in arr.iter().take(max) {
        lines.push(format_symbol_information(s));
    }
    if arr.len() > max {
        lines.push(format!(
            "  … 另有 {} 条（已按 max_results 截断）",
            arr.len() - max
        ));
    }
    Ok(lines.join("\n"))
}

pub(super) fn format_lsp_hover(resp: &Value) -> Result<String, String> {
    const TITLE: &str = "rust_analyzer hover";
    if let Some(err) = resp.get("error") {
        return Err(format!("{TITLE}: LSP error: {err}"));
    }
    let Some(result) = resp.get("result") else {
        return Ok(format!("{TITLE}: (空结果)"));
    };
    if result.is_null() {
        return Ok(format!("{TITLE}: (无内容)"));
    }
    let mut out = format!("{TITLE}:\n");
    if let Some(contents) = result.get("contents") {
        out.push_str(&format_hover_contents(contents));
    } else {
        out.push_str("(无 contents 字段)\n");
    }
    if let Some(range) = result.get("range")
        && let Some(pos) = range_start_1based_line_col(range)
    {
        out.push_str(&format!("\n--- range 起始（1-based 行:列）: {pos}\n"));
    }
    Ok(out.trim_end().to_string())
}

fn format_hover_contents(v: &Value) -> String {
    match v {
        Value::Array(a) => a
            .iter()
            .map(format_hover_piece)
            .collect::<Vec<_>>()
            .join("\n---\n"),
        _ => format_hover_piece(v),
    }
}

fn format_hover_piece(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            if let Some(kind) = o.get("kind").and_then(|x| x.as_str()) {
                let val = o.get("value").and_then(|x| x.as_str()).unwrap_or("");
                format!("[{kind}]\n{val}")
            } else if let Some(lang) = o.get("language").and_then(|x| x.as_str()) {
                let val = o.get("value").and_then(|x| x.as_str()).unwrap_or("");
                format!("```{lang}\n{val}\n```")
            } else {
                truncate_str(&v.to_string(), 4000)
            }
        }
        _ => truncate_str(&v.to_string(), 4000),
    }
}

pub(super) fn format_lsp_document_symbols(
    resp: &Value,
    max_symbols: usize,
) -> Result<String, String> {
    const TITLE: &str = "rust_analyzer document_symbol";
    if let Some(err) = resp.get("error") {
        return Err(format!("{TITLE}: LSP error: {err}"));
    }
    let result = resp.get("result");
    let Some(result) = result else {
        return Ok(format!("{TITLE}: (空结果)"));
    };
    let Some(arr) = result.as_array() else {
        return Ok(format!(
            "{TITLE}: (非数组: {})",
            truncate_str(&result.to_string(), 160)
        ));
    };
    if arr.is_empty() {
        return Ok(format!("{TITLE}: (无符号)"));
    }
    let mut lines = vec![format!("{TITLE}:")];
    let mut rem = max_symbols;
    if arr.first().is_some_and(|x| x.get("location").is_some()) {
        for s in arr {
            if rem == 0 {
                lines.push("  ... (已达 max_symbols)".to_string());
                break;
            }
            lines.push(format_symbol_information(s));
            rem -= 1;
        }
    } else {
        let mut truncated = false;
        for s in arr {
            append_document_symbol_lines(s, 0, &mut lines, &mut rem);
            if rem == 0 {
                truncated = true;
                break;
            }
        }
        if truncated {
            lines.push("  ... (已达 max_symbols)".to_string());
        }
    }
    Ok(lines.join("\n"))
}

fn format_symbol_information(v: &Value) -> String {
    let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("?");
    let kind = v
        .get("kind")
        .and_then(|x| x.as_u64())
        .map(lsp_symbol_kind_name)
        .unwrap_or("?");
    let loc = v.get("location");
    let uri = loc
        .and_then(|l| l.get("uri"))
        .and_then(|x| x.as_str())
        .unwrap_or("?");
    let pos = loc
        .and_then(|l| l.get("range"))
        .and_then(range_start_1based_line_col)
        .unwrap_or_else(|| "?".to_string());
    format!("  {} [{}] {} @ {}", name, kind, uri_to_brief_path(uri), pos)
}

fn append_document_symbol_lines(
    sym: &Value,
    depth: usize,
    lines: &mut Vec<String>,
    rem: &mut usize,
) {
    if *rem == 0 {
        return;
    }
    let name = sym.get("name").and_then(|x| x.as_str()).unwrap_or("?");
    let kind = sym
        .get("kind")
        .and_then(|x| x.as_u64())
        .map(lsp_symbol_kind_name)
        .unwrap_or("?");
    let range = sym.get("selectionRange").or_else(|| sym.get("range"));
    let pos = range
        .and_then(range_start_1based_line_col)
        .unwrap_or_else(|| "?".to_string());
    let indent = "  ".repeat(depth);
    lines.push(format!("{indent}{name} [{kind}] @ {pos}"));
    *rem -= 1;
    if let Some(children) = sym.get("children").and_then(|x| x.as_array()) {
        for ch in children {
            append_document_symbol_lines(ch, depth + 1, lines, rem);
            if *rem == 0 {
                break;
            }
        }
    }
}

fn range_start_1based_line_col(v: &Value) -> Option<String> {
    let s = v.get("start")?;
    let l = s.get("line").and_then(|x| x.as_u64()).unwrap_or(0) + 1;
    let c = s.get("character").and_then(|x| x.as_u64()).unwrap_or(0) + 1;
    Some(format!("{l}:{c}"))
}

fn lsp_symbol_kind_name(k: u64) -> &'static str {
    match k {
        1 => "File",
        2 => "Module",
        3 => "Namespace",
        4 => "Package",
        5 => "Class",
        6 => "Method",
        7 => "Property",
        8 => "Field",
        9 => "Constructor",
        10 => "Enum",
        11 => "Interface",
        12 => "Function",
        13 => "Variable",
        14 => "Constant",
        15 => "String",
        16 => "Number",
        17 => "Boolean",
        18 => "Array",
        19 => "Object",
        20 => "Key",
        21 => "Null",
        22 => "EnumMember",
        23 => "Struct",
        24 => "Event",
        25 => "Operator",
        26 => "TypeParameter",
        _ => "Unknown",
    }
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

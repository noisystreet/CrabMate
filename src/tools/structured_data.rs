//! JSON / YAML / TOML / CSV / TSV：解析校验、类 JSON Pointer / 点号路径查询、结构化 diff（与 `git_diff` 互补）。
//! 表格类与按行工具 `table_text` 互补：此处将整表载入为 JSON 模型后做路径查询与键级 diff。

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use serde_json::Value as JsonValue;

use super::ToolContext;
use super::file;
use crate::workspace_changelist::record_file_state_after_write;

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const DEFAULT_DIFF_MAX_LINES: usize = 200;
const ABS_DIFF_MAX_LINES: usize = 2000;
const PREVIEW_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataFormat {
    Json,
    Yaml,
    Toml,
    Csv,
    Tsv,
}

fn detect_format(path: &str, explicit: Option<&str>) -> Result<DataFormat, String> {
    if let Some(f) = explicit {
        let f = f.trim().to_lowercase();
        return match f.as_str() {
            "auto" => detect_from_path(path),
            "json" => Ok(DataFormat::Json),
            "yaml" | "yml" => Ok(DataFormat::Yaml),
            "toml" => Ok(DataFormat::Toml),
            "csv" => Ok(DataFormat::Csv),
            "tsv" => Ok(DataFormat::Tsv),
            _ => Err(format!(
                "不支持的 format：{}（可用 auto/json/yaml/toml/csv/tsv）",
                f
            )),
        };
    }
    detect_from_path(path)
}

fn detect_from_path(path: &str) -> Result<DataFormat, String> {
    let lower = path.to_lowercase();
    if lower.ends_with(".json") {
        return Ok(DataFormat::Json);
    }
    if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        return Ok(DataFormat::Yaml);
    }
    if lower.ends_with(".toml") {
        return Ok(DataFormat::Toml);
    }
    if lower.ends_with(".csv") {
        return Ok(DataFormat::Csv);
    }
    if lower.ends_with(".tsv") {
        return Ok(DataFormat::Tsv);
    }
    Err("无法从扩展名推断格式，请显式传 format（json / yaml / yml / toml / csv / tsv）".to_string())
}

fn parse_has_header(v: &JsonValue) -> bool {
    v.get("has_header")
        .and_then(|x| x.as_bool())
        .unwrap_or(true)
}

fn unique_header_keys(header: &csv::StringRecord) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::with_capacity(header.len());
    for (i, h) in header.iter().enumerate() {
        let base = if h.trim().is_empty() {
            format!("column_{}", i)
        } else {
            h.trim().to_string()
        };
        let n = seen.entry(base.clone()).or_insert(0);
        *n += 1;
        let key = if *n == 1 {
            base
        } else {
            format!("{}__{}", base, n)
        };
        out.push(key);
    }
    out
}

/// 将 CSV/TSV 文本解析为 JSON：`has_header=true` 时为对象数组（列名来自首行），否则为「字符串数组」的数组。
fn tabular_text_to_json(text: &str, delim: u8, has_header: bool) -> Result<JsonValue, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());

    if has_header {
        let mut it = rdr.records();
        let header_rec = it
            .next()
            .transpose()
            .map_err(|e| format!("CSV/TSV 表头解析失败: {}", e))?;
        let Some(header_rec) = header_rec else {
            return Ok(JsonValue::Array(vec![]));
        };
        let keys = unique_header_keys(&header_rec);
        let mut rows = Vec::new();
        for result in it {
            let rec = result.map_err(|e| format!("CSV/TSV 数据行解析失败: {}", e))?;
            let mut map = serde_json::Map::new();
            for (i, k) in keys.iter().enumerate() {
                let cell = rec.get(i).unwrap_or("");
                map.insert(k.clone(), JsonValue::String(cell.to_string()));
            }
            rows.push(JsonValue::Object(map));
        }
        Ok(JsonValue::Array(rows))
    } else {
        let mut rows = Vec::new();
        for result in rdr.records() {
            let rec = result.map_err(|e| format!("CSV/TSV 行解析失败: {}", e))?;
            let arr: Vec<JsonValue> = rec
                .iter()
                .map(|s| JsonValue::String(s.to_string()))
                .collect();
            rows.push(JsonValue::Array(arr));
        }
        Ok(JsonValue::Array(rows))
    }
}

fn read_limited(path: &Path) -> Result<String, String> {
    let meta = fs::metadata(path).map_err(|e| format!("读取元数据失败: {}", e))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!(
            "文件过大（{} 字节），上限 {}",
            meta.len(),
            MAX_FILE_BYTES
        ));
    }
    fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))
}

fn parse_to_json(text: &str, fmt: DataFormat, has_header: bool) -> Result<JsonValue, String> {
    match fmt {
        DataFormat::Json => serde_json::from_str(text).map_err(|e| format!("JSON 解析错误: {}", e)),
        DataFormat::Yaml => serde_yaml::from_str(text).map_err(|e| format!("YAML 解析错误: {}", e)),
        DataFormat::Toml => {
            let tv: toml::Value =
                toml::from_str(text).map_err(|e| format!("TOML 解析错误: {}", e))?;
            Ok(toml_value_to_json(tv))
        }
        DataFormat::Csv => tabular_text_to_json(text, b',', has_header),
        DataFormat::Tsv => tabular_text_to_json(text, b'\t', has_header),
    }
}

fn toml_value_to_json(tv: toml::Value) -> JsonValue {
    match tv {
        toml::Value::String(s) => JsonValue::String(s),
        toml::Value::Integer(i) => JsonValue::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::String(f.to_string())),
        toml::Value::Boolean(b) => JsonValue::Bool(b),
        toml::Value::Datetime(d) => JsonValue::String(d.to_string()),
        toml::Value::Array(a) => JsonValue::Array(a.into_iter().map(toml_value_to_json).collect()),
        toml::Value::Table(t) => JsonValue::Object(
            t.into_iter()
                .map(|(k, v)| (k, toml_value_to_json(v)))
                .collect(),
        ),
    }
}

fn summarize_top_level(v: &JsonValue, max_keys: usize) -> String {
    match v {
        JsonValue::Object(m) => {
            let keys: Vec<_> = m.keys().take(max_keys).cloned().collect();
            let mut s = format!("顶层对象，共 {} 个键", m.len());
            if !keys.is_empty() {
                s.push_str(&format!("；示例键: {}", keys.join(", ")));
            }
            if m.len() > max_keys {
                s.push_str(" …");
            }
            s
        }
        JsonValue::Array(a) => format!("顶层数组，长度 {}", a.len()),
        _ => format!("顶层标量: {}", preview_value(v)),
    }
}

fn preview_value(v: &JsonValue) -> String {
    let s = serde_json::to_string(v).unwrap_or_else(|_| "<无法序列化>".to_string());
    if s.len() > PREVIEW_MAX_CHARS {
        format!("{}…", &s[..PREVIEW_MAX_CHARS])
    } else {
        s
    }
}

/// `query`：若以 `/` 开头则按 JSON Pointer（RFC 6901）；否则按 `.` 分段（段为纯数字则作数组下标）。
fn resolve_query_path<'a>(v: &'a JsonValue, query: &str) -> Option<&'a JsonValue> {
    let q = query.trim();
    if q.is_empty() {
        return Some(v);
    }
    if q.starts_with('/') {
        return v.pointer(q);
    }
    let mut cur = v;
    for seg in q.split('.').filter(|s| !s.is_empty()) {
        if let Ok(i) = seg.parse::<usize>() {
            cur = cur.get(i)?;
        } else {
            cur = cur.get(seg)?;
        }
    }
    Some(cur)
}

fn diff_recursive(
    base_path: &str,
    a: &JsonValue,
    b: &JsonValue,
    out: &mut Vec<String>,
    max_lines: usize,
) {
    if out.len() >= max_lines {
        return;
    }
    if a == b {
        return;
    }
    match (a, b) {
        (JsonValue::Object(oa), JsonValue::Object(ob)) => {
            let keys: BTreeSet<_> = oa.keys().chain(ob.keys()).cloned().collect();
            for k in keys {
                if out.len() >= max_lines {
                    break;
                }
                let seg = json_pointer_escape(&k);
                let p = if base_path.is_empty() {
                    format!("/{}", seg)
                } else {
                    format!("{}/{}", base_path, seg)
                };
                match (oa.get(&k), ob.get(&k)) {
                    (None, Some(vb)) => {
                        out.push(format!("仅 B 存在: {} = {}", p, preview_value(vb)));
                    }
                    (Some(va), None) => {
                        out.push(format!("仅 A 存在: {} = {}", p, preview_value(va)));
                    }
                    (Some(va), Some(vb)) => diff_recursive(&p, va, vb, out, max_lines),
                    (None, None) => {}
                }
            }
        }
        (JsonValue::Array(aa), JsonValue::Array(ab)) => {
            let n = aa.len().max(ab.len());
            for i in 0..n {
                if out.len() >= max_lines {
                    break;
                }
                let p = if base_path.is_empty() {
                    format!("/{}", i)
                } else {
                    format!("{}/{}", base_path, i)
                };
                match (aa.get(i), ab.get(i)) {
                    (None, Some(vb)) => {
                        out.push(format!("仅 B 存在: {} = {}", p, preview_value(vb)));
                    }
                    (Some(va), None) => {
                        out.push(format!("仅 A 存在: {} = {}", p, preview_value(va)));
                    }
                    (Some(va), Some(vb)) => diff_recursive(&p, va, vb, out, max_lines),
                    (None, None) => {}
                }
            }
        }
        _ => {
            out.push(format!(
                "值不同: {} — A={} B={}",
                if base_path.is_empty() { "/" } else { base_path },
                preview_value(a),
                preview_value(b)
            ));
        }
    }
}

fn json_pointer_escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn json_pointer_unescape(s: &str) -> String {
    s.replace("~1", "/").replace("~0", "~")
}

fn parse_patch_query_tokens(query: &str) -> Result<Vec<String>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let parts = if q.starts_with('/') {
        q.split('/')
            .skip(1)
            .map(json_pointer_unescape)
            .collect::<Vec<_>>()
    } else {
        q.split('.')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    };
    if parts.iter().any(|s| s.len() > 256) {
        return Err("query 路径片段过长（单段最多 256 字符）".to_string());
    }
    Ok(parts)
}

fn ensure_parent_for_set<'a>(
    root: &'a mut JsonValue,
    tokens: &[String],
    create_missing: bool,
) -> Result<(&'a mut JsonValue, String), String> {
    if tokens.is_empty() {
        return Ok((root, String::new()));
    }
    let mut cur = root;
    for seg in &tokens[..tokens.len() - 1] {
        match cur {
            JsonValue::Object(map) => {
                if !map.contains_key(seg) {
                    if !create_missing {
                        return Err(format!("中间路径不存在: {}", seg));
                    }
                    map.insert(seg.clone(), JsonValue::Object(serde_json::Map::new()));
                }
                cur = map
                    .get_mut(seg)
                    .ok_or_else(|| format!("中间路径不存在: {}", seg))?;
            }
            JsonValue::Array(arr) => {
                let idx = seg
                    .parse::<usize>()
                    .map_err(|_| format!("数组路径片段不是非负整数: {}", seg))?;
                if idx >= arr.len() {
                    if create_missing && idx == arr.len() {
                        arr.push(JsonValue::Object(serde_json::Map::new()));
                    } else {
                        return Err(format!("数组下标越界: {}（长度 {}）", idx, arr.len()));
                    }
                }
                let len_now = arr.len();
                let Some(next) = arr.get_mut(idx) else {
                    return Err(format!("数组下标越界: {}（长度 {}）", idx, len_now));
                };
                cur = next;
            }
            _ => return Err("中间路径不是 object/array，无法继续下钻".to_string()),
        }
    }
    Ok((cur, tokens.last().cloned().unwrap_or_default()))
}

fn set_value_at_path(
    root: &mut JsonValue,
    tokens: &[String],
    value: JsonValue,
    create_missing: bool,
) -> Result<(), String> {
    if tokens.is_empty() {
        *root = value;
        return Ok(());
    }
    let (parent, last) = ensure_parent_for_set(root, tokens, create_missing)?;
    match parent {
        JsonValue::Object(map) => {
            map.insert(last, value);
            Ok(())
        }
        JsonValue::Array(arr) => {
            let idx = last
                .parse::<usize>()
                .map_err(|_| format!("数组路径片段不是非负整数: {}", last))?;
            if idx < arr.len() {
                arr[idx] = value;
                Ok(())
            } else if create_missing && idx == arr.len() {
                arr.push(value);
                Ok(())
            } else {
                Err(format!("数组下标越界: {}（长度 {}）", idx, arr.len()))
            }
        }
        _ => Err("目标父节点不是 object/array，无法写入".to_string()),
    }
}

fn remove_value_at_path(root: &mut JsonValue, tokens: &[String]) -> Result<(), String> {
    if tokens.is_empty() {
        return Err("remove 不支持空路径（根节点）".to_string());
    }
    let (parent, last) = ensure_parent_for_set(root, tokens, false)?;
    match parent {
        JsonValue::Object(map) => {
            if map.remove(&last).is_some() {
                Ok(())
            } else {
                Err(format!("路径不存在: {}", last))
            }
        }
        JsonValue::Array(arr) => {
            let idx = last
                .parse::<usize>()
                .map_err(|_| format!("数组路径片段不是非负整数: {}", last))?;
            if idx < arr.len() {
                arr.remove(idx);
                Ok(())
            } else {
                Err(format!("数组下标越界: {}（长度 {}）", idx, arr.len()))
            }
        }
        _ => Err("目标父节点不是 object/array，无法删除".to_string()),
    }
}

fn json_to_toml(v: &JsonValue) -> Result<toml::Value, String> {
    Ok(match v {
        JsonValue::Null => {
            return Err("TOML 不支持 null，请改为字符串/数字/布尔或删除该键".to_string());
        }
        JsonValue::Bool(b) => toml::Value::Boolean(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(u) = n.as_u64() {
                if u > i64::MAX as u64 {
                    return Err("TOML integer 超出 i64 范围".to_string());
                }
                toml::Value::Integer(u as i64)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                return Err("不支持的数字类型".to_string());
            }
        }
        JsonValue::String(s) => toml::Value::String(s.clone()),
        JsonValue::Array(arr) => toml::Value::Array(
            arr.iter()
                .map(json_to_toml)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        JsonValue::Object(map) => {
            let mut t = toml::map::Map::new();
            for (k, vv) in map {
                t.insert(k.clone(), json_to_toml(vv)?);
            }
            toml::Value::Table(t)
        }
    })
}

fn serialize_by_format(v: &JsonValue, fmt: DataFormat) -> Result<String, String> {
    let mut out = match fmt {
        DataFormat::Json => serde_json::to_string_pretty(v).map_err(|e| e.to_string())?,
        DataFormat::Yaml => serde_yaml::to_string(v).map_err(|e| e.to_string())?,
        DataFormat::Toml => {
            let tv = json_to_toml(v)?;
            toml::to_string_pretty(&tv).map_err(|e| e.to_string())?
        }
        DataFormat::Csv | DataFormat::Tsv => {
            return Err("structured_patch 仅支持 json/yaml/toml，不支持 csv/tsv".to_string());
        }
    };
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// 校验并可选摘要顶层结构。
/// 参数：`path`，`format?`（auto/json/yaml/toml/csv/tsv），`has_header?`（仅 CSV/TSV；默认 true），`summarize?` 默认 true
pub fn structured_validate(args_json: &str, working_dir: &Path) -> String {
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|x| x.as_str()).map(str::trim) {
        Some(p) if !p.is_empty() => p,
        _ => return "错误：缺少 path".to_string(),
    };
    let fmt = v
        .get("format")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let summarize = v.get("summarize").and_then(|x| x.as_bool()).unwrap_or(true);
    let has_header = parse_has_header(&v);

    let abs = match file::resolve_for_read(working_dir, path) {
        Ok(p) => p,
        Err(e) => return format!("错误：{}", e),
    };
    let data_fmt = match detect_format(path, fmt) {
        Ok(f) => f,
        Err(e) => return format!("错误：{}", e),
    };
    let text = match read_limited(&abs) {
        Ok(t) => t,
        Err(e) => return format!("错误：{}", e),
    };
    match parse_to_json(&text, data_fmt, has_header) {
        Ok(jv) => {
            let mut out = format!("校验通过: {}\n格式: {:?}\n", path, data_fmt);
            if summarize {
                out.push_str(&summarize_top_level(&jv, 24));
                out.push('\n');
            }
            out.trim_end().to_string()
        }
        Err(e) => format!("校验失败: {}\n{}", path, e),
    }
}

/// 解析后按路径取值（JSON Pointer 或点号路径）。
/// 参数：`path`，`query`（必填），`format?`，`has_header?`（仅 CSV/TSV；默认 true）
pub fn structured_query(args_json: &str, working_dir: &Path) -> String {
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|x| x.as_str()).map(str::trim) {
        Some(p) if !p.is_empty() => p,
        _ => return "错误：缺少 path".to_string(),
    };
    let query = match v.get("query").and_then(|x| x.as_str()).map(str::trim) {
        Some(q) if !q.is_empty() => q,
        _ => return "错误：缺少 query（JSON Pointer 如 /a/b 或点号路径如 a.b）".to_string(),
    };
    let fmt = v
        .get("format")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let has_header = parse_has_header(&v);

    let abs = match file::resolve_for_read(working_dir, path) {
        Ok(p) => p,
        Err(e) => return format!("错误：{}", e),
    };
    let data_fmt = match detect_format(path, fmt) {
        Ok(f) => f,
        Err(e) => return format!("错误：{}", e),
    };
    let text = match read_limited(&abs) {
        Ok(t) => t,
        Err(e) => return format!("错误：{}", e),
    };
    let jv = match parse_to_json(&text, data_fmt, has_header) {
        Ok(j) => j,
        Err(e) => return format!("解析失败: {}", e),
    };
    match resolve_query_path(&jv, query) {
        Some(found) => {
            format!(
                "路径: {}\nquery: {}\n类型: {}\n值:\n{}",
                path,
                query,
                json_type_name(found),
                serde_json::to_string_pretty(found).unwrap_or_else(|_| preview_value(found))
            )
        }
        None => format!("路径不存在或中间节点缺失: file={} query={}", path, query),
    }
}

fn json_type_name(v: &JsonValue) -> &'static str {
    match v {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

/// 将两份文件解析为同一 JSON 模型后做键级差异（非文本 diff）。
/// 参数：`path_a`，`path_b`，`format?`（对两边使用同一解释；若 auto 则分别按扩展名推断），`has_header?`（仅 CSV/TSV；默认 true），`max_diff_lines?` 默认 200，上限 2000
pub fn structured_diff(args_json: &str, working_dir: &Path) -> String {
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path_a = match v.get("path_a").and_then(|x| x.as_str()).map(str::trim) {
        Some(p) if !p.is_empty() => p,
        _ => return "错误：缺少 path_a".to_string(),
    };
    let path_b = match v.get("path_b").and_then(|x| x.as_str()).map(str::trim) {
        Some(p) if !p.is_empty() => p,
        _ => return "错误：缺少 path_b".to_string(),
    };
    let fmt_override = v
        .get("format")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let max_lines = v
        .get("max_diff_lines")
        .and_then(|x| x.as_u64())
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_DIFF_MAX_LINES)
        .clamp(1, ABS_DIFF_MAX_LINES);
    let has_header = parse_has_header(&v);

    let abs_a = match file::resolve_for_read(working_dir, path_a) {
        Ok(p) => p,
        Err(e) => return format!("错误 path_a: {}", e),
    };
    let abs_b = match file::resolve_for_read(working_dir, path_b) {
        Ok(p) => p,
        Err(e) => return format!("错误 path_b: {}", e),
    };

    let fmt_a = match detect_format(path_a, fmt_override) {
        Ok(f) => f,
        Err(e) => return format!("错误：{}", e),
    };
    let fmt_b = if fmt_override.is_some() {
        fmt_a
    } else {
        match detect_format(path_b, None) {
            Ok(f) => f,
            Err(e) => return format!("错误 path_b 格式: {}", e),
        }
    };

    let text_a = match read_limited(&abs_a) {
        Ok(t) => t,
        Err(e) => return format!("错误：{}", e),
    };
    let text_b = match read_limited(&abs_b) {
        Ok(t) => t,
        Err(e) => return format!("错误：{}", e),
    };

    let jv_a = match parse_to_json(&text_a, fmt_a, has_header) {
        Ok(j) => j,
        Err(e) => return format!("解析 path_a 失败: {}", e),
    };
    let jv_b = match parse_to_json(&text_b, fmt_b, has_header) {
        Ok(j) => j,
        Err(e) => return format!("解析 path_b 失败: {}", e),
    };

    let mut lines = Vec::new();
    diff_recursive("", &jv_a, &jv_b, &mut lines, max_lines);

    let mut out = String::new();
    out.push_str(&format!(
        "结构化 diff: {} vs {}\n格式: A={:?} B={:?}\n",
        path_a, path_b, fmt_a, fmt_b
    ));
    if lines.is_empty() {
        out.push_str("结论: 解析后结构一致（或仅标量相同）。\n");
    } else {
        out.push_str(&format!("差异条目（最多 {} 行）:\n", max_lines));
        for line in &lines {
            out.push_str(line);
            out.push('\n');
        }
        if lines.len() >= max_lines {
            out.push_str("…（已达 max_diff_lines 上限，请缩小文件或提高上限）\n");
        }
    }
    out.trim_end().to_string()
}

/// 结构化补丁：对 JSON/YAML/TOML 进行 set/remove（默认 dry-run，写入需 confirm=true）。
pub fn structured_patch(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|x| x.as_str()).map(str::trim) {
        Some(p) if !p.is_empty() => p,
        _ => return "错误：缺少 path".to_string(),
    };
    let query = match v.get("query").and_then(|x| x.as_str()).map(str::trim) {
        Some(q) => q,
        None => return "错误：缺少 query（JSON Pointer 或点号路径）".to_string(),
    };
    let action = v
        .get("action")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("set");
    if action != "set" && action != "remove" {
        return "错误：action 仅支持 set/remove".to_string();
    }
    let create_missing = v
        .get("create_missing")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let dry_run = v.get("dry_run").and_then(|x| x.as_bool()).unwrap_or(true);
    let confirm = v.get("confirm").and_then(|x| x.as_bool()).unwrap_or(false);
    if !dry_run && !confirm {
        return "错误：structured_patch 写盘需 confirm=true；建议先 dry_run=true 预览".to_string();
    }
    let fmt = v
        .get("format")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let abs = match file::resolve_for_read(working_dir, path) {
        Ok(p) => p,
        Err(e) => return format!("错误：{}", e),
    };
    let data_fmt = match detect_format(path, fmt) {
        Ok(DataFormat::Csv | DataFormat::Tsv) => {
            return "错误：structured_patch 不支持 csv/tsv，请改用 table_text 或直接编辑"
                .to_string();
        }
        Ok(f) => f,
        Err(e) => return format!("错误：{}", e),
    };
    let text = match read_limited(&abs) {
        Ok(t) => t,
        Err(e) => return format!("错误：{}", e),
    };
    let mut jv = match parse_to_json(&text, data_fmt, true) {
        Ok(j) => j,
        Err(e) => return format!("解析失败: {}", e),
    };
    let tokens = match parse_patch_query_tokens(query) {
        Ok(t) => t,
        Err(e) => return format!("query 无效: {}", e),
    };

    let apply_result = if action == "set" {
        let Some(new_value) = v.get("value").cloned() else {
            return "错误：action=set 时必须提供 value".to_string();
        };
        set_value_at_path(&mut jv, &tokens, new_value, create_missing)
    } else {
        remove_value_at_path(&mut jv, &tokens)
    };
    if let Err(e) = apply_result {
        return format!("补丁失败: {}", e);
    }
    let serialized = match serialize_by_format(&jv, data_fmt) {
        Ok(s) => s,
        Err(e) => return format!("序列化失败: {}", e),
    };
    if dry_run {
        return format!(
            "structured_patch 预览成功（未写入）: path={} action={} query={}\n新文件大小: {} 字节",
            path,
            action,
            if query.is_empty() { "(root)" } else { query },
            serialized.len()
        );
    }
    let before = text.clone();
    if let Err(e) = fs::write(&abs, serialized.as_bytes()) {
        return format!("写入失败: {}", e);
    }
    record_file_state_after_write(ctx.workspace_changelist, working_dir, path, Some(before));
    format!(
        "structured_patch 已写入: path={} action={} query={}",
        path,
        action,
        if query.is_empty() { "(root)" } else { query }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_dot_and_pointer() {
        let j: JsonValue = serde_json::json!({"a":{"b":[null,{"c":42}]}});
        assert_eq!(
            resolve_query_path(&j, "/a/b/1/c").and_then(JsonValue::as_i64),
            Some(42)
        );
        assert_eq!(
            resolve_query_path(&j, "a.b.1.c").and_then(JsonValue::as_i64),
            Some(42)
        );
    }

    #[test]
    fn diff_finds_mismatch() {
        let a = serde_json::json!({"x":1,"y":{"z":2}});
        let b = serde_json::json!({"x":1,"y":{"z":3}});
        let mut lines = Vec::new();
        diff_recursive("", &a, &b, &mut lines, 50);
        assert!(lines.iter().any(|l| l.contains("z") && l.contains("不同")));
    }

    #[test]
    fn csv_with_header_to_json_and_query() {
        let text = "name,score\nAlice,10\nBob,20\n";
        let jv = tabular_text_to_json(text, b',', true).unwrap();
        assert_eq!(
            resolve_query_path(&jv, "/0/name").and_then(JsonValue::as_str),
            Some("Alice")
        );
        assert_eq!(
            resolve_query_path(&jv, "1.score").and_then(JsonValue::as_str),
            Some("20")
        );
    }

    #[test]
    fn csv_without_header_is_array_of_arrays() {
        let text = "a,b\n1,2\n";
        let jv = tabular_text_to_json(text, b',', false).unwrap();
        assert_eq!(
            resolve_query_path(&jv, "/0/1").and_then(JsonValue::as_str),
            Some("b")
        );
    }

    #[test]
    fn set_value_at_path_creates_missing_objects() {
        let mut j = serde_json::json!({});
        set_value_at_path(
            &mut j,
            &["a".to_string(), "b".to_string()],
            serde_json::json!(1),
            true,
        )
        .unwrap();
        assert_eq!(j.pointer("/a/b").and_then(JsonValue::as_i64), Some(1));
    }

    #[test]
    fn remove_value_at_path_removes_object_key() {
        let mut j = serde_json::json!({"x":{"y":2}});
        remove_value_at_path(&mut j, &["x".to_string(), "y".to_string()]).unwrap();
        assert!(j.pointer("/x/y").is_none());
    }
}

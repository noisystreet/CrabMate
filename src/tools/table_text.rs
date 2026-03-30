//! 工作区内 CSV / TSV / 简单分隔纯文本：预览、列数校验、按列筛选、列选择与数值聚合（有行数与输出上限）。

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use serde_json::Value;

use super::file;

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_INLINE_BYTES: usize = 256 * 1024;
/// 校验 / 聚合 / 筛选时最多扫描的数据行（不含表头）
const MAX_ROWS_SCAN: usize = 200_000;
const MAX_PREVIEW_ROWS: usize = 200;
const DEFAULT_PREVIEW_ROWS: usize = 20;
const MAX_OUTPUT_ROWS: usize = 10_000;
const DEFAULT_OUTPUT_ROWS: usize = 500;
const MAX_OUTPUT_BYTES: usize = 512 * 1024;
const MAX_MISMATCH_REPORT: usize = 100;
const MAX_CELL_BYTES: usize = 65_536;

fn parse_action(v: &Value) -> Result<&'static str, String> {
    let s = v
        .get("action")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "缺少 action".to_string())?
        .trim()
        .to_lowercase();
    match s.as_str() {
        "preview" => Ok("preview"),
        "validate" => Ok("validate"),
        "select_columns" => Ok("select_columns"),
        "filter_rows" => Ok("filter_rows"),
        "aggregate" => Ok("aggregate"),
        _ => Err(format!(
            "未知 action：{}（支持 preview、validate、select_columns、filter_rows、aggregate）",
            s
        )),
    }
}

fn parse_delimiter(v: &Value, _path_hint: Option<&str>) -> Result<u8, String> {
    let raw = v
        .get("delimiter")
        .and_then(|x| x.as_str())
        .unwrap_or("auto")
        .trim()
        .to_lowercase();
    match raw.as_str() {
        "auto" => Ok(0), // 占位：由调用方在打开字节后 sniff
        "comma" | "csv" => Ok(b','),
        "tab" | "tsv" => Ok(b'\t'),
        "semicolon" => Ok(b';'),
        "pipe" | "|" => Ok(b'|'),
        _ => Err(format!(
            "delimiter 仅支持 auto、comma/csv、tab/tsv、semicolon、pipe（当前：{}）",
            raw
        )),
    }
}

fn resolve_delimiter_byte(
    delim_flag: u8,
    path_hint: Option<&str>,
    sniff_from: &[u8],
) -> Result<u8, String> {
    if delim_flag != 0 {
        return Ok(delim_flag);
    }
    if let Some(p) = path_hint {
        let lower = p.to_lowercase();
        if lower.ends_with(".tsv") {
            return Ok(b'\t');
        }
        if lower.ends_with(".csv") {
            return Ok(b',');
        }
    }
    Ok(sniff_delimiter(sniff_from))
}

fn sniff_delimiter(data: &[u8]) -> u8 {
    let line: &[u8] = data
        .split(|&b| b == b'\n' || b == b'\r')
        .find(|l| !l.is_empty())
        .unwrap_or(data);
    let tabs = line.iter().filter(|&&b| b == b'\t').count();
    let commas = line.iter().filter(|&&b| b == b',').count();
    let semis = line.iter().filter(|&&b| b == b';').count();
    if tabs >= commas && tabs >= semis && tabs > 0 {
        b'\t'
    } else if semis > commas && semis > tabs {
        b';'
    } else {
        b','
    }
}

fn read_source_bytes(
    path: Option<&str>,
    text: Option<&str>,
    workspace: &Path,
) -> Result<Vec<u8>, String> {
    if let Some(p) = path {
        let p = p.trim();
        if !p.is_empty() {
            let pb = file::resolve_for_read(workspace, p)
                .map_err(|e| format!("错误：{}", e.user_message()))?;
            let meta = std::fs::metadata(&pb).map_err(|e| format!("读取元数据失败: {}", e))?;
            if meta.len() > MAX_FILE_BYTES {
                return Err(format!(
                    "文件过大：{} 字节，上限 {}",
                    meta.len(),
                    MAX_FILE_BYTES
                ));
            }
            let mut buf = Vec::new();
            let mut f = File::open(&pb).map_err(|e| format!("打开文件失败: {}", e))?;
            f.read_to_end(&mut buf)
                .map_err(|e| format!("读取文件失败: {}", e))?;
            return Ok(buf);
        }
    }
    if let Some(t) = text {
        if t.is_empty() {
            return Err("text 不能为空".to_string());
        }
        if t.len() > MAX_INLINE_BYTES {
            return Err(format!(
                "text 过长：{} 字节，上限 {}",
                t.len(),
                MAX_INLINE_BYTES
            ));
        }
        return Ok(t.as_bytes().to_vec());
    }
    Err("须提供 path（相对工作区）或 text（内联，上限 256KiB）".to_string())
}

fn parse_path_text(v: &Value) -> (Option<String>, Option<String>) {
    let path = v
        .get("path")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let text = v
        .get("text")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    (path, text)
}

fn csv_reader(data: &[u8], delim: u8) -> csv::Reader<io::Cursor<&[u8]>> {
    csv::ReaderBuilder::new()
        .delimiter(delim)
        .has_headers(false)
        .flexible(true)
        .from_reader(io::Cursor::new(data))
}

fn truncate_str(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        return s.to_string();
    }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n\n[输出已截断，共 {} 字节，上限 {} 字节]",
        &s[..end],
        s.len(),
        MAX_OUTPUT_BYTES
    )
}

fn cell_preview(s: &str) -> String {
    let t = s.trim();
    if t.len() <= 200 {
        return t.to_string();
    }
    let mut end = 200;
    while end > 0 && !t.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…（{} 字节，已截断预览）", &t[..end], t.len())
}

fn run_preview(
    data: &[u8],
    delim: u8,
    has_header: bool,
    preview_rows: usize,
) -> Result<String, String> {
    let pr = preview_rows.clamp(1, MAX_PREVIEW_ROWS);
    let mut rdr = csv_reader(data, delim);
    let mut out = String::new();
    out.push_str(&format!(
        "delimiter={} has_header={}\n",
        delim as char, has_header
    ));

    let mut it = rdr.records();
    if has_header {
        if let Some(rec) = it
            .next()
            .transpose()
            .map_err(|e| format!("解析表头失败: {}", e))?
        {
            let fields: Vec<String> = rec
                .iter()
                .map(|f| {
                    if f.len() > MAX_CELL_BYTES {
                        format!("（单元格过长 {} 字节）", f.len())
                    } else {
                        cell_preview(f)
                    }
                })
                .collect();
            out.push_str("[header] ");
            out.push_str(&fields.join(" | "));
            out.push('\n');
        } else {
            out.push_str("[header] （空文件）\n");
            return Ok(out);
        }
    }

    let mut shown = 0usize;
    let mut row_idx = 0usize;
    while shown < pr {
        let rec = match it.next() {
            None => break,
            Some(Err(e)) => return Err(format!("解析数据行失败: {}", e)),
            Some(Ok(r)) => r,
        };
        row_idx += 1;
        let ncols = rec.len();
        let fields: Vec<String> = rec
            .iter()
            .map(|f| {
                if f.len() > MAX_CELL_BYTES {
                    format!("（{} 字节）", f.len())
                } else {
                    cell_preview(f)
                }
            })
            .collect();
        out.push_str(&format!(
            "[{}] cols={} {}\n",
            row_idx,
            ncols,
            fields.join(" | ")
        ));
        shown += 1;
    }

    let mut rest = 0usize;
    let mut capped = false;
    for _ in it {
        rest += 1;
        if rest > MAX_ROWS_SCAN {
            capped = true;
            break;
        }
    }
    let tail = if capped {
        format!("≥{}", rest)
    } else {
        format!("{}", rest)
    };
    out.push_str(&format!(
        "（预览 {} 行数据；其后还有 {} 行未展开）\n",
        shown, tail
    ));
    Ok(out)
}

fn run_validate(data: &[u8], delim: u8, max_scan: usize) -> Result<String, String> {
    let cap = max_scan.clamp(1, MAX_ROWS_SCAN);
    let mut rdr = csv_reader(data, delim);
    let mut expected: Option<usize> = None;
    let mut row_num = 0usize;
    let mut mismatches: Vec<String> = Vec::new();
    let mut total = 0usize;

    for result in rdr.records() {
        let rec = result.map_err(|e| format!("第 {} 行解析失败: {}", row_num + 1, e))?;
        row_num += 1;
        let n = rec.len();
        match expected {
            None => expected = Some(n),
            Some(e) if e != n => {
                if mismatches.len() < MAX_MISMATCH_REPORT {
                    mismatches.push(format!("行 {}：期望 {} 列，实际 {} 列", row_num, e, n));
                }
            }
            _ => {}
        }
        total += 1;
        if total >= cap {
            break;
        }
    }

    let mut out = String::new();
    out.push_str(&format!(
        "delimiter={:?} 扫描数据行数（上限 {}）: {}\n",
        delim as char, cap, total
    ));
    if let Some(e) = expected {
        out.push_str(&format!("首行参照列数: {}\n", e));
    }
    if mismatches.is_empty() {
        out.push_str("列数一致（在扫描范围内）。\n");
    } else {
        out.push_str("列数不一致：\n");
        for m in &mismatches {
            out.push_str(m);
            out.push('\n');
        }
        if mismatches.len() >= MAX_MISMATCH_REPORT {
            out.push_str(&format!("（仅列出前 {} 处不一致）\n", MAX_MISMATCH_REPORT));
        }
    }
    Ok(out)
}

fn run_select_columns(
    data: &[u8],
    delim: u8,
    columns: &[usize],
    has_header: bool,
    max_out: usize,
) -> Result<String, String> {
    if columns.is_empty() {
        return Err("select_columns 需要非空 columns（0 起列下标）".to_string());
    }
    let max_out = max_out.clamp(1, MAX_OUTPUT_ROWS);
    let mut rdr = csv_reader(data, delim);
    let mut it = rdr.records();
    let mut out = String::new();

    if has_header
        && let Some(rec) = it
            .next()
            .transpose()
            .map_err(|e| format!("表头解析失败: {}", e))?
    {
        let fields: Vec<&str> = rec.iter().collect();
        let picked: Vec<String> = columns
            .iter()
            .map(|&i| {
                fields
                    .get(i)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("（缺列 {}）", i))
            })
            .collect();
        out.push_str(&picked.join("\t"));
        out.push('\n');
    }

    let mut written = 0usize;
    for result in it {
        let rec = result.map_err(|e| format!("解析行失败: {}", e))?;
        let fields: Vec<&str> = rec.iter().collect();
        let picked: Vec<String> = columns
            .iter()
            .map(|&i| fields.get(i).map(|s| s.to_string()).unwrap_or_default())
            .collect();
        out.push_str(&picked.join("\t"));
        out.push('\n');
        written += 1;
        if written >= max_out {
            out.push_str(&format!("（已截断至 {} 行，上限 {}）\n", written, max_out));
            break;
        }
    }
    Ok(truncate_str(&out))
}

fn run_filter_rows(
    data: &[u8],
    delim: u8,
    col: usize,
    equals: Option<&str>,
    contains: Option<&str>,
    has_header: bool,
    max_out: usize,
) -> Result<String, String> {
    if equals.is_none() && contains.is_none() {
        return Err("filter_rows 需要 equals 或 contains 之一".to_string());
    }
    let max_out = max_out.clamp(1, MAX_OUTPUT_ROWS);
    let mut rdr = csv_reader(data, delim);
    let mut it = rdr.records();

    if has_header {
        let _ = it
            .next()
            .transpose()
            .map_err(|e| format!("表头解析失败: {}", e))?;
    }

    let mut out = String::new();
    let mut written = 0usize;
    for result in it {
        let rec = result.map_err(|e| format!("解析行失败: {}", e))?;
        let cell = rec.get(col).unwrap_or("");
        let ok = match (equals, contains) {
            (Some(e), _) => cell == e,
            (None, Some(s)) => cell.contains(s),
            _ => false,
        };
        if !ok {
            continue;
        }
        let line = rec.iter().collect::<Vec<_>>().join("\t");
        out.push_str(&line);
        out.push('\n');
        written += 1;
        if written >= max_out {
            out.push_str(&format!("（已截断至 {} 行，上限 {}）\n", written, max_out));
            break;
        }
    }
    Ok(truncate_str(&out))
}

fn parse_f64_cell(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok()
}

fn run_aggregate(
    data: &[u8],
    delim: u8,
    col: usize,
    op: &str,
    has_header: bool,
    max_scan: usize,
) -> Result<String, String> {
    let cap = max_scan.clamp(1, MAX_ROWS_SCAN);
    let mut rdr = csv_reader(data, delim);
    let mut it = rdr.records();
    if has_header {
        let _ = it
            .next()
            .transpose()
            .map_err(|e| format!("表头解析失败: {}", e))?;
    }

    let mut scanned = 0usize;
    let mut sum = 0.0f64;
    let mut count_num = 0usize;
    let mut count_nonempty = 0usize;
    let mut min_v: Option<f64> = None;
    let mut max_v: Option<f64> = None;

    for result in it {
        let rec = result.map_err(|e| format!("解析行失败: {}", e))?;
        scanned += 1;
        if scanned > cap {
            break;
        }
        let cell = rec.get(col).unwrap_or("");
        if !cell.trim().is_empty() {
            count_nonempty += 1;
        }
        if let Some(v) = parse_f64_cell(cell) {
            count_num += 1;
            sum += v;
            min_v = Some(match min_v {
                None => v,
                Some(m) => m.min(v),
            });
            max_v = Some(match max_v {
                None => v,
                Some(m) => m.max(v),
            });
        }
    }

    let op = op.trim().to_lowercase();
    let summary = match op.as_str() {
        "count" | "count_non_empty" => format!("非空单元格数: {}", count_nonempty),
        "count_numeric" => format!("可解析为数字的单元格数: {}", count_num),
        "sum" => {
            if count_num == 0 {
                "sum: （无数值）".to_string()
            } else {
                format!("sum: {}", sum)
            }
        }
        "mean" | "avg" => {
            if count_num == 0 {
                "mean: （无数值）".to_string()
            } else {
                format!("mean: {}", sum / count_num as f64)
            }
        }
        "min" => format!("min: {:?}", min_v),
        "max" => format!("max: {:?}", max_v),
        _ => {
            return Err(format!(
                "aggregate.op 支持 count、count_non_empty、count_numeric、sum、mean、min、max（当前：{}）",
                op
            ));
        }
    };

    Ok(format!(
        "delimiter={:?} column={} 扫描行数上限={} 实际扫描={}\n{}",
        delim as char,
        col,
        cap,
        scanned.min(cap),
        summary
    ))
}

fn parse_usize(v: &Value, key: &str, default: usize, max: usize) -> Result<usize, String> {
    let n = match v.get(key) {
        None => default,
        Some(x) => x.as_u64().ok_or_else(|| format!("{} 须为非负整数", key))? as usize,
    };
    if n > max {
        return Err(format!("{} 不能超过 {}", key, max));
    }
    Ok(n)
}

fn parse_column_indices(v: &Value) -> Result<Vec<usize>, String> {
    let arr = v
        .get("columns")
        .and_then(|x| x.as_array())
        .ok_or_else(|| "select_columns 需要 columns 数组（0 起整数下标）".to_string())?;
    if arr.is_empty() {
        return Err("columns 不能为空".to_string());
    }
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        let n = x
            .as_u64()
            .ok_or_else(|| "columns 元素须为非负整数".to_string())? as usize;
        out.push(n);
    }
    Ok(out)
}

/// 执行 `table_text` 工具。
pub fn run(args_json: &str, workspace_root: &Path) -> String {
    let v: Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效：{}", e),
    };

    let action = match parse_action(&v) {
        Ok(a) => a,
        Err(e) => return e,
    };

    let (path, text) = parse_path_text(&v);
    let path_str = path.as_deref();
    let text_str = text.as_deref();

    let bytes = match read_source_bytes(path_str, text_str, workspace_root) {
        Ok(b) => b,
        Err(e) => return e,
    };

    let delim_flag = match parse_delimiter(&v, path_str) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let delim = match resolve_delimiter_byte(delim_flag, path_str, &bytes) {
        Ok(d) => d,
        Err(e) => return e,
    };

    let has_header = v
        .get("has_header")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);

    match action {
        "preview" => {
            let pr = match parse_usize(&v, "preview_rows", DEFAULT_PREVIEW_ROWS, MAX_PREVIEW_ROWS) {
                Ok(n) => n,
                Err(e) => return e,
            };
            match run_preview(&bytes, delim, has_header, pr) {
                Ok(s) => truncate_str(&s),
                Err(e) => e,
            }
        }
        "validate" => {
            let max_scan = match parse_usize(&v, "max_rows_scan", MAX_ROWS_SCAN, MAX_ROWS_SCAN) {
                Ok(n) => n,
                Err(e) => return e,
            };
            match run_validate(&bytes, delim, max_scan) {
                Ok(s) => s,
                Err(e) => e,
            }
        }
        "select_columns" => {
            let cols = match parse_column_indices(&v) {
                Ok(c) => c,
                Err(e) => return e,
            };
            let max_out =
                match parse_usize(&v, "max_output_rows", DEFAULT_OUTPUT_ROWS, MAX_OUTPUT_ROWS) {
                    Ok(n) => n,
                    Err(e) => return e,
                };
            match run_select_columns(&bytes, delim, &cols, has_header, max_out) {
                Ok(s) => s,
                Err(e) => e,
            }
        }
        "filter_rows" => {
            let col = match v.get("column").and_then(|x| x.as_u64()) {
                Some(n) => n as usize,
                None => return "filter_rows 需要 column（非负整数，0 起）".to_string(),
            };
            let equals = v.get("equals").and_then(|x| x.as_str());
            let contains = v.get("contains").and_then(|x| x.as_str());
            let max_out =
                match parse_usize(&v, "max_output_rows", DEFAULT_OUTPUT_ROWS, MAX_OUTPUT_ROWS) {
                    Ok(n) => n,
                    Err(e) => return e,
                };
            match run_filter_rows(&bytes, delim, col, equals, contains, has_header, max_out) {
                Ok(s) => s,
                Err(e) => e,
            }
        }
        "aggregate" => {
            let col = match v.get("column").and_then(|x| x.as_u64()) {
                Some(n) => n as usize,
                None => return "aggregate 需要 column（非负整数，0 起）".to_string(),
            };
            let op = match v.get("op").and_then(|x| x.as_str()) {
                Some(s) => s,
                None => {
                    return "aggregate 需要 op（如 sum、mean、min、max、count_non_empty）"
                        .to_string();
                }
            };
            let max_scan = match parse_usize(&v, "max_rows_scan", MAX_ROWS_SCAN, MAX_ROWS_SCAN) {
                Ok(n) => n,
                Err(e) => return e,
            };
            match run_aggregate(&bytes, delim, col, op, has_header, max_scan) {
                Ok(s) => s,
                Err(e) => e,
            }
        }
        _ => "内部错误：未知 action".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_consistent() {
        let data = b"a,b\n1,2\n3,4\n";
        let s = run_validate(data, b',', 100).unwrap();
        assert!(s.contains("列数一致"));
    }

    #[test]
    fn validate_mismatch() {
        let data = b"a,b\n1,2\n3\n";
        let s = run_validate(data, b',', 100).unwrap();
        assert!(s.contains("不一致"));
    }

    #[test]
    fn aggregate_sum() {
        let data = b"h1,h2\n1,10\n2,20\n";
        let s = run_aggregate(data, b',', 1, "sum", true, 100).unwrap();
        assert!(s.contains("30"));
    }
}

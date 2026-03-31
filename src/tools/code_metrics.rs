//! 代码度量与分析工具：行数统计（tokei 库）、依赖图、覆盖率报告解析

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use super::output_util;

const MAX_OUTPUT_LINES: usize = 600;
const EXCLUDED_DIRS: &[&str] = &["target", "node_modules", "vendor", "dist", "build", ".git"];

// ── code_stats：代码行数统计 ────────────────────────────────

pub fn code_stats(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let path = v
        .get("path")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    if path.contains("..") || path.starts_with('/') {
        return "错误：path 不安全（禁止 .. 与绝对路径）".to_string();
    }
    let target = workspace_root.join(path);
    if !target.exists() {
        return format!("错误：路径 {} 不存在", path);
    }

    let format = v
        .get("format")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .unwrap_or("table");

    let paths = &[target.to_string_lossy().to_string()];
    let excluded: Vec<&str> = EXCLUDED_DIRS.to_vec();
    let config = tokei::Config::default();
    let mut languages = tokei::Languages::new();
    languages.get_statistics(paths, &excluded, &config);

    let mut sorted: Vec<_> = languages
        .iter()
        .filter(|(_, lang)| lang.code > 0 || lang.comments > 0 || lang.blanks > 0)
        .collect();
    sorted.sort_by(|a, b| b.1.code.cmp(&a.1.code));

    if sorted.is_empty() {
        return format!("路径 {} 下未找到可识别的源码文件", path);
    }

    let total_files: usize = sorted.iter().map(|(_, l)| l.reports.len()).sum();
    let total_code: usize = sorted.iter().map(|(_, l)| l.code).sum();
    let total_comments: usize = sorted.iter().map(|(_, l)| l.comments).sum();
    let total_blanks: usize = sorted.iter().map(|(_, l)| l.blanks).sum();
    let total_lines = total_code + total_comments + total_blanks;

    if format == "json" {
        let entries: Vec<serde_json::Value> = sorted
            .iter()
            .map(|(lang_type, lang)| {
                serde_json::json!({
                    "language": format!("{}", lang_type),
                    "files": lang.reports.len(),
                    "lines": lang.code + lang.comments + lang.blanks,
                    "blank": lang.blanks,
                    "comment": lang.comments,
                    "code": lang.code
                })
            })
            .collect();
        let result = serde_json::json!({
            "total_files": total_files,
            "total_lines": total_lines,
            "total_code": total_code,
            "total_comments": total_comments,
            "total_blanks": total_blanks,
            "languages": entries
        });
        return match serde_json::to_string_pretty(&result) {
            Ok(s) => output_util::truncate_output_lines(&s, max_output_len, MAX_OUTPUT_LINES),
            Err(e) => format!("JSON 序列化错误：{}", e),
        };
    }

    let mut out = String::new();
    out.push_str(&format!("代码统计（tokei 库）：{}\n", path));
    out.push_str(&format!(
        "{:<20} {:>6} {:>10} {:>8} {:>8} {:>8}\n",
        "Language", "Files", "Lines", "Blank", "Comment", "Code"
    ));
    out.push_str(&"-".repeat(64));
    out.push('\n');
    for (lang_type, lang) in &sorted {
        let lines = lang.code + lang.comments + lang.blanks;
        out.push_str(&format!(
            "{:<20} {:>6} {:>10} {:>8} {:>8} {:>8}\n",
            format!("{}", lang_type),
            lang.reports.len(),
            lines,
            lang.blanks,
            lang.comments,
            lang.code
        ));
    }
    out.push_str(&"-".repeat(64));
    out.push('\n');
    out.push_str(&format!(
        "{:<20} {:>6} {:>10} {:>8} {:>8} {:>8}\n",
        "Total", total_files, total_lines, total_blanks, total_comments, total_code
    ));

    output_util::truncate_output_lines(&out, max_output_len, MAX_OUTPUT_LINES)
}

// ── dependency_graph：依赖关系可视化 ────────────────────────

pub fn dependency_graph(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let format = v
        .get("format")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .unwrap_or("mermaid");
    let depth = v.get("depth").and_then(|x| x.as_u64()).unwrap_or(1) as usize;
    let kind = v
        .get("kind")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .unwrap_or("auto");

    if workspace_root.join("Cargo.toml").is_file()
        && (kind == "auto" || kind == "rust" || kind == "cargo")
    {
        return cargo_dep_graph(workspace_root, format, depth, max_output_len);
    }
    if workspace_root.join("go.mod").is_file() && (kind == "auto" || kind == "go") {
        return go_dep_graph(workspace_root, format, max_output_len);
    }
    if (workspace_root.join("package.json").is_file()
        || workspace_root.join("package.json").is_file())
        && (kind == "auto" || kind == "npm" || kind == "node")
    {
        return npm_dep_graph(workspace_root, format, max_output_len);
    }

    "未检测到 Cargo.toml / go.mod / package.json，无法生成依赖图".to_string()
}

fn cargo_dep_graph(
    workspace_root: &Path,
    format: &str,
    depth: usize,
    max_output_len: usize,
) -> String {
    let mut cmd = Command::new("cargo");
    cmd.arg("tree")
        .arg("--depth")
        .arg(depth.min(10).to_string())
        .arg("--no-dedupe")
        .current_dir(workspace_root);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return format!("cargo tree 执行失败：{}", e),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);

    if format == "mermaid" {
        return cargo_tree_to_mermaid(&stdout, max_output_len);
    }
    if format == "dot" {
        return cargo_tree_to_dot(&stdout, max_output_len);
    }
    format!(
        "cargo tree (depth={})：\n{}",
        depth,
        output_util::truncate_output_lines(stdout.trim_end(), max_output_len, MAX_OUTPUT_LINES)
    )
}

fn is_tree_decoration(c: char) -> bool {
    matches!(c, ' ' | '│' | '├' | '└' | '─' | '|')
}

fn pkg_safe_id(name: &str) -> String {
    name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
}

fn cargo_tree_to_mermaid(tree_output: &str, max_output_len: usize) -> String {
    let mut lines = vec!["graph TD".to_string()];
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();

    for line in tree_output.lines() {
        let trimmed = line.trim_start_matches(is_tree_decoration);
        let indent = line.len() - trimmed.len();
        let level = indent / 4;
        let pkg_name = trimmed.split_whitespace().next().unwrap_or("").to_string();
        if pkg_name.is_empty() || pkg_name == "(*)" {
            continue;
        }
        let safe_id = pkg_safe_id(&pkg_name);

        while stack.last().is_some_and(|(l, _)| *l >= level) {
            stack.pop();
        }
        if let Some((_, parent)) = stack.last() {
            let edge = (parent.clone(), safe_id.clone());
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
        stack.push((level, safe_id));
    }

    for (from, to) in &edges {
        lines.push(format!("    {} --> {}", from, to));
    }

    let result = lines.join("\n");
    format!(
        "Mermaid 依赖图（Cargo）：\n```mermaid\n{}\n```",
        output_util::truncate_output_lines(&result, max_output_len, MAX_OUTPUT_LINES)
    )
}

fn cargo_tree_to_dot(tree_output: &str, max_output_len: usize) -> String {
    let mut lines = vec!["digraph deps {".to_string(), "    rankdir=LR;".to_string()];
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();

    for line in tree_output.lines() {
        let trimmed = line.trim_start_matches(is_tree_decoration);
        let indent = line.len() - trimmed.len();
        let level = indent / 4;
        let pkg_name = trimmed.split_whitespace().next().unwrap_or("").to_string();
        if pkg_name.is_empty() || pkg_name == "(*)" {
            continue;
        }
        let safe_id = pkg_safe_id(&pkg_name);

        while stack.last().is_some_and(|(l, _)| *l >= level) {
            stack.pop();
        }
        if let Some((_, parent)) = stack.last() {
            let edge = (parent.clone(), safe_id.clone());
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
        stack.push((level, safe_id));
    }

    for (from, to) in &edges {
        lines.push(format!("    \"{}\" -> \"{}\";", from, to));
    }
    lines.push("}".to_string());

    let result = lines.join("\n");
    format!(
        "DOT 依赖图（Cargo）：\n{}",
        output_util::truncate_output_lines(&result, max_output_len, MAX_OUTPUT_LINES)
    )
}

fn go_dep_graph(workspace_root: &Path, format: &str, max_output_len: usize) -> String {
    let output = match Command::new("go")
        .arg("list")
        .arg("-m")
        .arg("all")
        .current_dir(workspace_root)
        .output()
    {
        Ok(o) => o,
        Err(e) => return format!("go list -m all 执行失败：{}", e),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mods: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

    if format == "mermaid" || format == "dot" {
        let root = mods.first().copied().unwrap_or("module");
        let root_name = root.split_whitespace().next().unwrap_or("module");
        let root_id = sanitize_id(root_name);
        let mut lines = if format == "mermaid" {
            vec!["graph TD".to_string()]
        } else {
            vec!["digraph deps {".to_string(), "    rankdir=LR;".to_string()]
        };
        for m in mods.iter().skip(1).take(100) {
            let dep_name = m.split_whitespace().next().unwrap_or("");
            if dep_name.is_empty() {
                continue;
            }
            let dep_id = sanitize_id(dep_name);
            if format == "mermaid" {
                lines.push(format!("    {} --> {}", root_id, dep_id));
            } else {
                lines.push(format!("    \"{}\" -> \"{}\";", root_id, dep_id));
            }
        }
        if format == "dot" {
            lines.push("}".to_string());
        }
        let result = lines.join("\n");
        let label = if format == "mermaid" {
            "Mermaid"
        } else {
            "DOT"
        };
        return format!(
            "{} 依赖图（Go）：\n{}",
            label,
            output_util::truncate_output_lines(&result, max_output_len, MAX_OUTPUT_LINES)
        );
    }

    format!(
        "Go 依赖列表（共 {} 个模块）：\n{}",
        mods.len(),
        output_util::truncate_output_lines(stdout.trim_end(), max_output_len, MAX_OUTPUT_LINES)
    )
}

fn npm_dep_graph(workspace_root: &Path, format: &str, max_output_len: usize) -> String {
    let dir = if workspace_root.join("package.json").is_file() {
        workspace_root.to_path_buf()
    } else {
        workspace_root.join("frontend")
    };

    let output = match Command::new("npm")
        .arg("ls")
        .arg("--depth=1")
        .arg("--json")
        .current_dir(&dir)
        .output()
    {
        Ok(o) => o,
        Err(e) => return format!("npm ls 执行失败：{}", e),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);

    if format == "raw" || (format != "mermaid" && format != "dot") {
        let text_out = match Command::new("npm")
            .arg("ls")
            .arg("--depth=1")
            .current_dir(&dir)
            .output()
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(_) => stdout.to_string(),
        };
        return format!(
            "npm 依赖树：\n{}",
            output_util::truncate_output_lines(
                text_out.trim_end(),
                max_output_len,
                MAX_OUTPUT_LINES
            )
        );
    }

    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => {
            return format!(
                "npm ls --json 解析失败，原始输出：\n{}",
                output_util::truncate_output_lines(
                    stdout.trim_end(),
                    max_output_len,
                    MAX_OUTPUT_LINES
                )
            );
        }
    };
    let root_name = parsed
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("project");
    let root_id = sanitize_id(root_name);
    let deps = parsed.get("dependencies").and_then(|d| d.as_object());

    let mut lines = if format == "mermaid" {
        vec!["graph TD".to_string()]
    } else {
        vec!["digraph deps {".to_string(), "    rankdir=LR;".to_string()]
    };

    if let Some(deps_map) = deps {
        for (dep_name, _) in deps_map.iter().take(100) {
            let dep_id = sanitize_id(dep_name);
            if format == "mermaid" {
                lines.push(format!("    {} --> {}", root_id, dep_id));
            } else {
                lines.push(format!("    \"{}\" -> \"{}\";", root_id, dep_id));
            }
        }
    }
    if format == "dot" {
        lines.push("}".to_string());
    }

    let result = lines.join("\n");
    let label = if format == "mermaid" {
        "Mermaid"
    } else {
        "DOT"
    };
    format!(
        "{} 依赖图（npm）：\n{}",
        label,
        output_util::truncate_output_lines(&result, max_output_len, MAX_OUTPUT_LINES)
    )
}

fn sanitize_id(name: &str) -> String {
    name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
        .trim_start_matches('_')
        .to_string()
}

// ── coverage_report：覆盖率报告解析 ────────────────────────

pub fn coverage_report(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let path = match v.get("path").and_then(|x| x.as_str()).map(str::trim) {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => {
            return auto_detect_coverage(workspace_root, max_output_len);
        }
    };
    if path.contains("..") || path.starts_with('/') {
        return "错误：path 不安全（禁止 .. 与绝对路径）".to_string();
    }

    let format = v
        .get("format")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .unwrap_or("auto");

    let full = workspace_root.join(&path);
    if !full.is_file() {
        return format!("错误：覆盖率文件 {} 不存在", path);
    }

    let content = match std::fs::read_to_string(&full) {
        Ok(c) => c,
        Err(e) => return format!("读取覆盖率文件失败：{}", e),
    };

    let actual_format = if format != "auto" {
        format.to_string()
    } else {
        detect_coverage_format(&path, &content)
    };

    match actual_format.as_str() {
        "lcov" => parse_lcov(&content, max_output_len),
        "tarpaulin" | "tarpaulin_json" => parse_tarpaulin_json(&content, max_output_len),
        "cobertura" => parse_cobertura_summary(&content, max_output_len),
        _ => {
            let preview = output_util::truncate_output_lines(&content, max_output_len / 2, 50);
            format!(
                "覆盖率文件 {}（格式：{}）前 50 行：\n{}",
                path, actual_format, preview
            )
        }
    }
}

fn auto_detect_coverage(workspace_root: &Path, max_output_len: usize) -> String {
    let candidates = [
        "lcov.info",
        "coverage/lcov.info",
        "coverage.json",
        "tarpaulin-report.json",
        "coverage/tarpaulin-report.json",
        "coverage/cobertura.xml",
        "cobertura.xml",
        "coverage/coverage.json",
    ];
    for c in &candidates {
        let full = workspace_root.join(c);
        if full.is_file() {
            let content = match std::fs::read_to_string(&full) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let fmt = detect_coverage_format(c, &content);
            let result = match fmt.as_str() {
                "lcov" => parse_lcov(&content, max_output_len),
                "tarpaulin" | "tarpaulin_json" => parse_tarpaulin_json(&content, max_output_len),
                "cobertura" => parse_cobertura_summary(&content, max_output_len),
                _ => continue,
            };
            return format!("自动检测覆盖率文件：{}\n{}", c, result);
        }
    }
    "未找到覆盖率文件。支持的文件：lcov.info、tarpaulin-report.json、cobertura.xml。请用 path 参数指定路径。".to_string()
}

fn detect_coverage_format(path: &str, content: &str) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".info") || content.starts_with("TN:") || content.starts_with("SF:") {
        return "lcov".to_string();
    }
    if lower.ends_with(".xml") && (content.contains("<coverage") || content.contains("cobertura")) {
        return "cobertura".to_string();
    }
    if lower.ends_with(".json")
        && content.contains("\"covered\"")
        && content.contains("\"coverable\"")
    {
        return "tarpaulin".to_string();
    }
    "unknown".to_string()
}

fn parse_lcov(content: &str, max_output_len: usize) -> String {
    let mut files: Vec<(String, usize, usize)> = Vec::new();
    let mut current_file = String::new();
    let mut lines_found: usize = 0;
    let mut lines_hit: usize = 0;

    for line in content.lines() {
        if let Some(sf) = line.strip_prefix("SF:") {
            current_file = sf.trim().to_string();
        } else if let Some(lf) = line.strip_prefix("LF:") {
            lines_found = lf.trim().parse().unwrap_or(0);
        } else if let Some(lh) = line.strip_prefix("LH:") {
            lines_hit = lh.trim().parse().unwrap_or(0);
        } else if line == "end_of_record" {
            if !current_file.is_empty() {
                files.push((current_file.clone(), lines_found, lines_hit));
            }
            current_file.clear();
            lines_found = 0;
            lines_hit = 0;
        }
    }

    if files.is_empty() {
        return "LCOV 文件为空或解析无结果".to_string();
    }

    let total_found: usize = files.iter().map(|(_, f, _)| f).sum();
    let total_hit: usize = files.iter().map(|(_, _, h)| h).sum();
    let pct = if total_found > 0 {
        total_hit as f64 / total_found as f64 * 100.0
    } else {
        0.0
    };

    let mut out = format!(
        "LCOV 覆盖率摘要：{:.1}%（{}/{} 行）\n\n",
        pct, total_hit, total_found
    );
    out.push_str(&format!(
        "{:<50} {:>8} {:>8} {:>8}\n",
        "File", "Lines", "Hit", "Pct"
    ));
    out.push_str(&"-".repeat(78));
    out.push('\n');

    files.sort_by(|a, b| {
        let pa = if a.1 > 0 {
            a.2 as f64 / a.1 as f64
        } else {
            1.0
        };
        let pb = if b.1 > 0 {
            b.2 as f64 / b.1 as f64
        } else {
            1.0
        };
        pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
    });

    for (file, found, hit) in &files {
        let fp = if *found > 0 {
            *hit as f64 / *found as f64 * 100.0
        } else {
            100.0
        };
        let short = if file.len() > 48 {
            format!("...{}", &file[file.len() - 45..])
        } else {
            file.clone()
        };
        out.push_str(&format!(
            "{:<50} {:>8} {:>8} {:>7.1}%\n",
            short, found, hit, fp
        ));
    }

    output_util::truncate_output_lines(&out, max_output_len, MAX_OUTPUT_LINES)
}

fn parse_tarpaulin_json(content: &str, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(e) => return format!("Tarpaulin JSON 解析失败：{}", e),
    };

    let mut file_stats: HashMap<String, (usize, usize)> = HashMap::new();

    if let Some(files) = v.get("files").and_then(|f| f.as_array()) {
        for f in files {
            let path = f.get("path").and_then(|p| p.as_str()).unwrap_or("?");
            let covered = f.get("covered").and_then(|c| c.as_u64()).unwrap_or(0) as usize;
            let coverable = f.get("coverable").and_then(|c| c.as_u64()).unwrap_or(0) as usize;
            file_stats.insert(path.to_string(), (coverable, covered));
        }
    } else if let Some(covered) = v.get("covered").and_then(|c| c.as_u64()) {
        let coverable = v.get("coverable").and_then(|c| c.as_u64()).unwrap_or(0);
        let pct = if coverable > 0 {
            covered as f64 / coverable as f64 * 100.0
        } else {
            0.0
        };
        return format!(
            "Tarpaulin 覆盖率：{:.1}%（{}/{} 行）",
            pct, covered, coverable
        );
    }

    if file_stats.is_empty() {
        return "Tarpaulin JSON：无文件级覆盖数据".to_string();
    }

    let total_coverable: usize = file_stats.values().map(|(c, _)| c).sum();
    let total_covered: usize = file_stats.values().map(|(_, h)| h).sum();
    let pct = if total_coverable > 0 {
        total_covered as f64 / total_coverable as f64 * 100.0
    } else {
        0.0
    };

    let mut out = format!(
        "Tarpaulin 覆盖率摘要：{:.1}%（{}/{} 行）\n\n",
        pct, total_covered, total_coverable
    );
    let mut sorted: Vec<_> = file_stats.into_iter().collect();
    sorted.sort_by(|a, b| {
        let pa = if a.1.0 > 0 {
            a.1.1 as f64 / a.1.0 as f64
        } else {
            1.0
        };
        let pb = if b.1.0 > 0 {
            b.1.1 as f64 / b.1.0 as f64
        } else {
            1.0
        };
        pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
    });

    for (file, (coverable, covered)) in &sorted {
        let fp = if *coverable > 0 {
            *covered as f64 / *coverable as f64 * 100.0
        } else {
            100.0
        };
        let short = if file.len() > 48 {
            format!("...{}", &file[file.len() - 45..])
        } else {
            file.clone()
        };
        out.push_str(&format!("{:<50} {:.1}%\n", short, fp));
    }

    output_util::truncate_output_lines(&out, max_output_len, MAX_OUTPUT_LINES)
}

fn parse_cobertura_summary(content: &str, max_output_len: usize) -> String {
    let mut line_rate = None;
    let mut branch_rate = None;

    for line in content.lines().take(20) {
        if line.contains("line-rate=")
            && let Some(val) = extract_xml_attr(line, "line-rate")
        {
            line_rate = val.parse::<f64>().ok();
        }
        if line.contains("branch-rate=")
            && let Some(val) = extract_xml_attr(line, "branch-rate")
        {
            branch_rate = val.parse::<f64>().ok();
        }
        if line_rate.is_some() {
            break;
        }
    }

    match (line_rate, branch_rate) {
        (Some(lr), Some(br)) => {
            format!(
                "Cobertura 覆盖率：行覆盖 {:.1}%，分支覆盖 {:.1}%",
                lr * 100.0,
                br * 100.0
            )
        }
        (Some(lr), None) => {
            format!("Cobertura 覆盖率：行覆盖 {:.1}%", lr * 100.0)
        }
        _ => {
            let preview = output_util::truncate_output_lines(content, max_output_len / 2, 30);
            format!(
                "Cobertura XML 未找到 line-rate 属性，前 30 行：\n{}",
                preview
            )
        }
    }
}

fn extract_xml_attr<'a>(line: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{}=\"", attr);
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_id() {
        assert_eq!(sanitize_id("my-crate"), "my_crate");
        assert_eq!(sanitize_id("@scope/pkg"), "scope_pkg");
    }

    #[test]
    fn test_detect_lcov() {
        assert_eq!(detect_coverage_format("lcov.info", "TN:\nSF:foo"), "lcov");
        assert_eq!(detect_coverage_format("x.info", "SF:bar"), "lcov");
    }

    #[test]
    fn test_detect_tarpaulin() {
        let content = r#"{"covered":10,"coverable":20}"#;
        assert_eq!(detect_coverage_format("report.json", content), "tarpaulin");
    }

    #[test]
    fn test_parse_lcov_basic() {
        let lcov = "TN:\nSF:src/main.rs\nLF:100\nLH:80\nend_of_record\n";
        let result = parse_lcov(lcov, 10000);
        assert!(result.contains("80.0%"));
        assert!(result.contains("main.rs"));
    }

    #[test]
    fn test_extract_xml_attr() {
        let line = r#"<coverage line-rate="0.85" branch-rate="0.70">"#;
        assert_eq!(extract_xml_attr(line, "line-rate"), Some("0.85"));
        assert_eq!(extract_xml_attr(line, "branch-rate"), Some("0.70"));
    }

    #[test]
    fn test_cobertura_summary() {
        let xml = r#"<?xml version="1.0"?><coverage line-rate="0.85" branch-rate="0.70">"#;
        let result = parse_cobertura_summary(xml, 10000);
        assert!(result.contains("85.0%"));
        assert!(result.contains("70.0%"));
    }
}

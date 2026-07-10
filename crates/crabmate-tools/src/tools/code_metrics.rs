//! 代码度量与分析工具：行数统计（可选 tokei / 内置 walk）、依赖图、覆盖率报告解析

use std::collections::HashMap;
use std::path::Path;

use super::output_util;
use super::tool_param_types::{
    CodeStatsArgs, CodeStatsFormat, CoverageReportArgs, CoverageReportFormat,
};
use crate::project_metrics;

pub(super) const MAX_OUTPUT_LINES: usize = 600;

// ── code_stats：代码行数统计 ────────────────────────────────

pub fn code_stats(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: CodeStatsArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 code_stats 形状不一致: {e}"),
    };
    let path = args
        .path
        .as_deref()
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

    let format = match args.format.unwrap_or_default() {
        CodeStatsFormat::Table => "table",
        CodeStatsFormat::Json => "json",
    };

    let stats = project_metrics::gather_workspace_code_stats(
        &target,
        project_metrics::DEFAULT_EXCLUDED_DIRS,
    );
    if stats.languages.is_empty() {
        return format!("路径 {} 下未找到可识别的源码文件", path);
    }

    let total_files = stats.total_files();
    let total_code = stats.total_code();
    let total_comments = stats.total_comments();
    let total_blanks = stats.total_blanks();
    let total_lines = stats.total_lines();

    if format == "json" {
        let entries: Vec<serde_json::Value> = stats
            .languages
            .iter()
            .map(|lang| {
                serde_json::json!({
                    "language": lang.language,
                    "files": lang.files,
                    "lines": lang.total_lines(),
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

    let source_label = if cfg!(feature = "project_metrics") {
        "tokei 库"
    } else {
        "内置扩展名统计"
    };
    let mut out = String::new();
    out.push_str(&format!("代码统计（{}）：{}\n", source_label, path));
    out.push_str(&format!(
        "{:<20} {:>6} {:>10} {:>8} {:>8} {:>8}\n",
        "Language", "Files", "Lines", "Blank", "Comment", "Code"
    ));
    out.push_str(&"-".repeat(64));
    out.push('\n');
    for lang in &stats.languages {
        out.push_str(&format!(
            "{:<20} {:>6} {:>10} {:>8} {:>8} {:>8}\n",
            lang.language,
            lang.files,
            lang.total_lines(),
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

#[path = "code_metrics_dependency_graph.rs"]
mod code_metrics_dependency_graph;

pub use code_metrics_dependency_graph::dependency_graph;

// ── coverage_report：覆盖率报告解析 ────────────────────────

pub fn coverage_report(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: CoverageReportArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 coverage_report 形状不一致: {e}"),
    };
    let path = match args.path.as_deref().map(str::trim) {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => {
            return auto_detect_coverage(workspace_root, max_output_len);
        }
    };
    if path.contains("..") || path.starts_with('/') {
        return "错误：path 不安全（禁止 .. 与绝对路径）".to_string();
    }

    let format = match args.format.unwrap_or_default() {
        CoverageReportFormat::Auto => "auto",
        CoverageReportFormat::Lcov => "lcov",
        CoverageReportFormat::Tarpaulin => "tarpaulin",
        CoverageReportFormat::TarpaulinJson => "tarpaulin_json",
        CoverageReportFormat::Cobertura => "cobertura",
    };

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
            let suffix: String = file
                .chars()
                .rev()
                .take(45)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("...{}", suffix)
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
            let suffix: String = file
                .chars()
                .rev()
                .take(45)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("...{}", suffix)
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

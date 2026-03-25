//! 工作区 TODO/FIXME 扫描工具

use std::fs;
use std::path::Path;

const DEFAULT_MARKERS: &[&str] = &["TODO", "FIXME", "HACK", "XXX"];
const MAX_RESULTS: usize = 200;
const MAX_LINE_PREVIEW: usize = 200;

pub fn run(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let markers: Vec<String> = v
        .get("markers")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.to_uppercase())
                .collect()
        })
        .unwrap_or_else(|| DEFAULT_MARKERS.iter().map(|s| s.to_string()).collect());

    let paths: Vec<String> = v
        .get("paths")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(|| vec![".".to_string()]);

    let exclude: Vec<String> = v
        .get("exclude")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(|| {
            vec![
                "target".into(),
                "node_modules".into(),
                ".git".into(),
                "vendor".into(),
                "dist".into(),
                "build".into(),
            ]
        });

    let mut results: Vec<String> = Vec::new();
    for rel_path in &paths {
        if rel_path.contains("..") || rel_path.starts_with('/') {
            continue;
        }
        let abs = working_dir.join(rel_path);
        scan_dir(&abs, working_dir, &markers, &exclude, &mut results);
        if results.len() >= MAX_RESULTS {
            break;
        }
    }

    if results.is_empty() {
        return format!("未找到标记（{}）", markers.join(", "));
    }
    let total = results.len();
    let shown = total.min(MAX_RESULTS);
    let mut out = format!("找到 {} 处标记（{}）：\n\n", total, markers.join(", "));
    for r in results.iter().take(shown) {
        out.push_str(r);
        out.push('\n');
    }
    if total > shown {
        out.push_str(&format!("\n... 已截断，仅显示前 {} 条", shown));
    }
    out
}

fn scan_dir(
    dir: &Path,
    root: &Path,
    markers: &[String],
    exclude: &[String],
    results: &mut Vec<String>,
) {
    if results.len() >= MAX_RESULTS {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if results.len() >= MAX_RESULTS {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if exclude.contains(&name) || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            scan_dir(&path, root, markers, exclude, results);
        } else if path.is_file() {
            scan_file(&path, root, markers, results);
        }
    }
}

fn scan_file(path: &Path, root: &Path, markers: &[String], results: &mut Vec<String>) {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let text_exts = [
        "rs", "py", "js", "ts", "tsx", "jsx", "go", "c", "cpp", "h", "hpp", "java", "kt", "rb",
        "sh", "yaml", "yml", "toml", "md", "txt", "css", "scss", "html", "vue", "svelte",
    ];
    if !text_exts.contains(&ext) {
        return;
    }
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let rel = path.strip_prefix(root).unwrap_or(path);
    for (line_num, line) in content.lines().enumerate() {
        if results.len() >= MAX_RESULTS {
            return;
        }
        let upper = line.to_uppercase();
        for marker in markers {
            if upper.contains(marker.as_str()) {
                let preview = if line.len() > MAX_LINE_PREVIEW {
                    format!("{}...", &line[..MAX_LINE_PREVIEW])
                } else {
                    line.to_string()
                };
                results.push(format!(
                    "{}:{}:  {}",
                    rel.display(),
                    line_num + 1,
                    preview.trim()
                ));
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_finds_todo_in_temp_file() {
        let dir = std::env::temp_dir().join("crabmate_todo_scan_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("test.rs"),
            "fn main() {\n    // TODO: fix this\n    // normal comment\n    // FIXME: urgent\n}\n",
        )
        .unwrap();
        let out = run(r#"{"paths":["."]}"#, &dir);
        assert!(out.contains("TODO"), "should find TODO, got: {}", out);
        assert!(out.contains("FIXME"), "should find FIXME, got: {}", out);
        assert!(out.contains("找到 2 处"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_empty_dir() {
        let dir = std::env::temp_dir().join("crabmate_todo_scan_empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let out = run(r#"{}"#, &dir);
        assert!(out.contains("未找到"));
        let _ = fs::remove_dir_all(&dir);
    }
}

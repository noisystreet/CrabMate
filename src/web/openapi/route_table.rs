//! 从 axum `.route(` 源码收集 path+method，供 OpenAPI 对照测试。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const HTTP_OPS: &[&str] = &["get", "put", "post", "delete", "patch", "options", "head"];

pub(super) fn axum_route_ops_from_source(manifest_dir: &Path) -> BTreeSet<(String, String)> {
    let mut files = vec![
        manifest_dir.join("src/web/server.rs"),
        manifest_dir.join("src/cm_web_host/routes/web_ui.rs"),
    ];
    collect_rs_files(
        &manifest_dir.join("src/web/routes"),
        &["e2e_fixtures"],
        &mut files,
    );
    let mut ops = BTreeSet::new();
    for path in files {
        let src = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (route_path, methods) in parse_route_invocations(&src) {
            for method in methods {
                ops.insert((route_path.clone(), method));
            }
        }
    }
    ops
}

fn collect_rs_files(dir: &Path, skip_dir_names: &[&str], out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("dirent {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            let skip = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| skip_dir_names.contains(&n));
            if !skip {
                collect_rs_files(&path, skip_dir_names, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn parse_route_invocations(src: &str) -> Vec<(String, BTreeSet<String>)> {
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(rel) = src[search_from..].find(".route(") {
        let inner_start = search_from + rel + ".route(".len();
        let Some(close) = closing_paren_offset(&src[inner_start..]) else {
            break;
        };
        let inner = &src[inner_start..inner_start + close];
        if let Some((path, methods)) = parse_one_route_args(inner) {
            out.push((path, methods));
        }
        search_from = inner_start + close + 1;
    }
    out
}

fn closing_paren_offset(s: &str) -> Option<usize> {
    let mut depth = 1;
    let mut in_str = false;
    let mut escape = false;
    for (i, c) in s.char_indices() {
        if in_str {
            if escape {
                escape = false;
                continue;
            }
            match c {
                '\\' => escape = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_one_route_args(inner: &str) -> Option<(String, BTreeSet<String>)> {
    let trimmed = inner.trim_start();
    let rest = trimmed.strip_prefix('"')?;
    let end = rest.find('"')?;
    let path = rest[..end].to_string();
    if !path.starts_with('/') {
        return None;
    }
    Some((path, http_methods_in(&rest[end + 1..])))
}

fn http_methods_in(args_after_path: &str) -> BTreeSet<String> {
    let mut methods = BTreeSet::new();
    for op in HTTP_OPS {
        let token = format!("{op}(");
        let mut from = 0;
        while let Some(rel) = args_after_path[from..].find(&token) {
            let at = from + rel;
            let ok_boundary = at == 0
                || args_after_path
                    .as_bytes()
                    .get(at - 1)
                    .is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'_');
            if ok_boundary {
                methods.insert((*op).to_string());
            }
            from = at + token.len();
        }
    }
    methods
}

pub(super) fn openapi_path_ops(spec: &serde_json::Value) -> BTreeSet<(String, String)> {
    let mut ops = BTreeSet::new();
    let paths = spec["paths"].as_object().expect("OpenAPI paths object");
    for (path, item) in paths {
        let Some(obj) = item.as_object() else {
            continue;
        };
        for key in obj.keys() {
            if HTTP_OPS.contains(&key.as_str()) {
                ops.insert((path.clone(), key.clone()));
            }
        }
    }
    ops
}

#[cfg(test)]
mod parse_tests {
    use super::parse_route_invocations;

    #[test]
    fn parse_multiline_route_with_chained_methods() {
        let src = r#"
            Router::new()
                .route(
                    "/workspace/file",
                    get(read)
                        .post(write)
                        .delete(del),
                )
                .route("/chat", post(chat_handler));
        "#;
        let parsed = parse_route_invocations(src);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "/workspace/file");
        assert_eq!(
            parsed[0].1.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["delete", "get", "post"]
        );
        assert_eq!(parsed[1].0, "/chat");
        assert!(parsed[1].1.contains("post"));
    }
}

//! Node.js / npm / npx 生态工具

use std::path::Path;
use std::process::Command;

use super::ToolContext;
use super::output_util;
use super::test_result_cache::{
    TestCacheKey, TestCacheKind, fingerprint_npm_package_dir, npm_test_args_fingerprint,
    store_cached, try_get_cached, wrap_cache_hit,
};

const MAX_OUTPUT_LINES: usize = 800;

pub fn npm_install(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let subdir = safe_subdir(&v);
    let ci = v.get("ci").and_then(|x| x.as_bool()).unwrap_or(false);
    let production = v
        .get("production")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    if let Some(e) = check_subdir(subdir) {
        return e;
    }
    let dir = workspace_root.join(subdir);
    if !dir.join("package.json").is_file() {
        return format!("npm install: 跳过（{}/package.json 不存在）", subdir);
    }

    let mut cmd = Command::new("npm");
    if ci {
        cmd.arg("ci");
    } else {
        cmd.arg("install");
    }
    if production {
        cmd.arg("--production");
    }
    cmd.current_dir(&dir);
    let title = if ci { "npm ci" } else { "npm install" };
    run_and_format(cmd, max_output_len, title)
}

pub fn npm_run(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    ctx: &ToolContext<'_>,
) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let subdir = safe_subdir(&v);
    let script = match v.get("script").and_then(|x| x.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s,
        _ => return "错误：缺少 script 参数".to_string(),
    };
    if script.contains(char::is_whitespace) {
        return "错误：script 参数无效（不能包含空白字符）".to_string();
    }

    if let Some(e) = check_subdir(subdir) {
        return e;
    }
    let dir = workspace_root.join(subdir);
    if !dir.join("package.json").is_file() {
        return format!("npm run {}: 跳过（{}/package.json 不存在）", script, subdir);
    }

    let extra_args: Vec<String> = v
        .get("args")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let run = || {
        let mut cmd = Command::new("npm");
        cmd.arg("run").arg(script);
        if !extra_args.is_empty() {
            cmd.arg("--");
            for a in &extra_args {
                cmd.arg(a);
            }
        }
        cmd.current_dir(&dir);
        run_and_format(cmd, max_output_len, &format!("npm run {}", script))
    };

    if ctx.test_result_cache_enabled
        && script == "test"
        && let Some(inputs_fp) = fingerprint_npm_package_dir(workspace_root, subdir)
    {
        let args_fp = npm_test_args_fingerprint(subdir, script, &extra_args);
        let key = TestCacheKey {
            workspace_root: workspace_root.to_path_buf(),
            kind: TestCacheKind::NpmTest {
                package_subdir: subdir.to_string(),
            },
            args_fingerprint: args_fp,
            inputs_fingerprint: inputs_fp.clone(),
        };
        if let Some(hit) = try_get_cached(
            ctx.test_result_cache_enabled,
            ctx.test_result_cache_max_entries,
            &key,
        ) {
            return wrap_cache_hit(&inputs_fp, &hit);
        }
        let out = run();
        store_cached(
            ctx.test_result_cache_enabled,
            ctx.test_result_cache_max_entries,
            key,
            out.clone(),
        );
        return out;
    }

    run()
}

pub fn npx_run(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let subdir = safe_subdir(&v);
    let package = match v.get("package").and_then(|x| x.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s,
        _ => return "错误：缺少 package 参数".to_string(),
    };
    if package.contains("..") || package.starts_with('/') {
        return "错误：package 参数不安全".to_string();
    }

    if let Some(e) = check_subdir(subdir) {
        return e;
    }
    let dir = workspace_root.join(subdir);
    if !dir.join("package.json").is_file() {
        return format!("npx {}: 跳过（{}/package.json 不存在）", package, subdir);
    }

    let extra_args: Vec<&str> = v
        .get("args")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();

    let mut cmd = Command::new("npx");
    cmd.arg("--yes").arg(package);
    for a in &extra_args {
        cmd.arg(a);
    }
    cmd.current_dir(&dir);
    run_and_format(cmd, max_output_len, &format!("npx {}", package))
}

pub fn tsc_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let subdir = safe_subdir(&v);
    let project = v
        .get("project")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let strict = v.get("strict").and_then(|x| x.as_bool()).unwrap_or(false);

    if let Some(e) = check_subdir(subdir) {
        return e;
    }
    let dir = workspace_root.join(subdir);
    if !dir.join("package.json").is_file() && !dir.join("tsconfig.json").is_file() {
        return format!(
            "tsc: 跳过（{} 下未找到 package.json 或 tsconfig.json）",
            subdir
        );
    }

    let mut cmd = Command::new("npx");
    cmd.arg("tsc");
    if let Some(p) = project {
        if p.contains("..") || p.starts_with('/') {
            return "错误：project 参数不安全".to_string();
        }
        cmd.arg("-p").arg(p);
    } else {
        cmd.arg("-b");
    }
    cmd.arg("--noEmit");
    if strict {
        cmd.arg("--strict");
    }
    cmd.current_dir(&dir);
    run_and_format(cmd, max_output_len, "npx tsc --noEmit")
}

fn safe_subdir(v: &serde_json::Value) -> &str {
    v.get("subdir")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".")
}

fn check_subdir(subdir: &str) -> Option<String> {
    if subdir.starts_with('/') || subdir.contains("..") {
        Some("错误：subdir 必须是工作区内相对路径，且不能包含 ..".to_string())
    } else {
        None
    }
}

fn run_and_format(mut cmd: Command, max_output_len: usize, title: &str) -> String {
    match cmd.output() {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut body = String::new();
            if !stdout.trim().is_empty() {
                body.push_str(stdout.trim_end());
            }
            if !stderr.trim().is_empty() {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(stderr.trim_end());
            }
            if body.is_empty() {
                body.push_str("(无输出)");
            }
            format!(
                "{} (exit={}):\n{}",
                title,
                status,
                output_util::truncate_output_lines(&body, max_output_len, MAX_OUTPUT_LINES)
            )
        }
        Err(e) => format!("{}: 无法启动命令（{}）", title, e),
    }
}

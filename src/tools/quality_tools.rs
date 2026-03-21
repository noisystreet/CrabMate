//! 质量与一致性：组合多项检查（fmt/clippy/test/前端），便于 agent 一键拉齐。

use std::path::Path;

use super::{cargo_tools, ci_tools, frontend_tools};

/// 按开关组合运行多项质量检查；默认仅 Rust 侧 fmt + clippy（轻量）。
pub fn quality_workspace(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };

    let run_cargo_fmt_check = v
        .get("run_cargo_fmt_check")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let run_cargo_clippy = v
        .get("run_cargo_clippy")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let run_cargo_test = v
        .get("run_cargo_test")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let run_frontend_lint = v
        .get("run_frontend_lint")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let run_frontend_prettier_check = v
        .get("run_frontend_prettier_check")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let fail_fast = v.get("fail_fast").and_then(|x| x.as_bool()).unwrap_or(true);
    let summary_only = v
        .get("summary_only")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    if !run_cargo_fmt_check
        && !run_cargo_clippy
        && !run_cargo_test
        && !run_frontend_lint
        && !run_frontend_prettier_check
    {
        return "错误：至少启用一项检查（run_cargo_fmt_check / run_cargo_clippy / run_cargo_test / run_frontend_lint / run_frontend_prettier_check）".to_string();
    }

    let mut sections: Vec<String> = Vec::new();
    let mut summary: Vec<(&'static str, &'static str)> = Vec::new();

    if run_cargo_fmt_check {
        let r = ci_tools::cargo_fmt_check_tool("{}", workspace_root, max_output_len);
        let failed = section_failed(&r);
        summary.push(("cargo fmt --check", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            push_skipped(
                &mut summary,
                run_cargo_clippy,
                run_cargo_test,
                run_frontend_lint,
                run_frontend_prettier_check,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("cargo fmt --check", "skipped"));
    }

    if run_cargo_clippy {
        let r = if workspace_root.join("Cargo.toml").is_file() {
            cargo_tools::cargo_clippy(r#"{"all_targets":true}"#, workspace_root, max_output_len)
        } else {
            "cargo clippy: 跳过（未找到 Cargo.toml）".to_string()
        };
        let failed = section_failed(&r);
        summary.push(("cargo clippy", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            push_skipped(
                &mut summary,
                false,
                run_cargo_test,
                run_frontend_lint,
                run_frontend_prettier_check,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("cargo clippy", "skipped"));
    }

    if run_cargo_test {
        let r = if workspace_root.join("Cargo.toml").is_file() {
            cargo_tools::cargo_test("{}", workspace_root, max_output_len)
        } else {
            "cargo test: 跳过（未找到 Cargo.toml）".to_string()
        };
        let failed = section_failed(&r);
        summary.push(("cargo test", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            push_skipped(
                &mut summary,
                false,
                false,
                run_frontend_lint,
                run_frontend_prettier_check,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("cargo test", "skipped"));
    }

    if run_frontend_lint {
        let r = frontend_tools::frontend_lint(
            r#"{"subdir":"frontend","script":"lint"}"#,
            workspace_root,
            max_output_len,
        );
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push(("frontend lint", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            push_skipped(&mut summary, false, false, false, run_frontend_prettier_check);
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("frontend lint", "skipped"));
    }

    if run_frontend_prettier_check {
        let r =
            frontend_tools::frontend_prettier_check(r#"{"subdir":"frontend"}"#, workspace_root, max_output_len);
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push((
            "frontend prettier --check",
            if failed { "failed" } else { "passed" },
        ));
        sections.push(r);
    } else {
        summary.push(("frontend prettier --check", "skipped"));
    }

    build_output(&summary, &sections, summary_only)
}

fn section_failed(text: &str) -> bool {
    if text.contains("执行失败（") || text.contains(": 执行失败（") {
        return true;
    }
    if let Some(line) = text.lines().next() {
        if let Some(rest) = line.strip_prefix("cargo fmt --check (exit=") {
            return parse_exit_nonzero(rest);
        }
        if line.starts_with("cargo ") && line.contains("(exit=")
            && let Some(idx) = line.find("(exit=") {
                return parse_exit_nonzero(&line[idx..]);
            }
        if line.contains("npm run") && line.contains("(exit=")
            && let Some(idx) = line.find("(exit=") {
                return parse_exit_nonzero(&line[idx..]);
            }
        if line.contains("npx prettier") && line.contains("(exit=")
            && let Some(idx) = line.find("(exit=") {
                return parse_exit_nonzero(&line[idx..]);
            }
    }
    text.contains("error: could not compile") || text.contains("error[E")
}

fn parse_exit_nonzero(s: &str) -> bool {
    // s 形如 "(exit=1):"、"1):"（fmt 首行 strip 后）等
    let s = s.trim();
    let rest = s
        .strip_prefix("(exit=")
        .unwrap_or(s);
    let code = rest
        .split(':')
        .next()
        .and_then(|x| x.trim_end_matches(')').parse::<i32>().ok())
        .unwrap_or(-1);
    code != 0
}

fn push_skipped(
    summary: &mut Vec<(&'static str, &'static str)>,
    clippy: bool,
    test: bool,
    fe_lint: bool,
    fe_fmt: bool,
) {
    if clippy {
        summary.push(("cargo clippy", "skipped"));
    }
    if test {
        summary.push(("cargo test", "skipped"));
    }
    if fe_lint {
        summary.push(("frontend lint", "skipped"));
    }
    if fe_fmt {
        summary.push(("frontend prettier --check", "skipped"));
    }
}

fn build_output(
    summary: &[(&'static str, &'static str)],
    sections: &[String],
    summary_only: bool,
) -> String {
    let line = summary
        .iter()
        .map(|(n, s)| format!("{}={}", n, s))
        .collect::<Vec<_>>()
        .join(", ");
    let head = format!("quality_workspace 汇总: {}\n", line);
    if summary_only {
        return head.trim_end().to_string();
    }
    let body = sections.join("\n\n====================\n\n");
    format!("{}\n{}", head, body.trim())
}

//! 质量与一致性：组合多项检查（Rust / 前端 / 可选 Python），便于 agent 一键拉齐。

use std::path::Path;

use super::{cargo_tools, ci_tools, container_tools, frontend_tools, jvm_tools, python_tools};

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
    let run_ruff_check = v
        .get("run_ruff_check")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let run_pytest = v
        .get("run_pytest")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let run_mypy = v.get("run_mypy").and_then(|x| x.as_bool()).unwrap_or(false);
    let run_maven_compile = v
        .get("run_maven_compile")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let run_maven_test = v
        .get("run_maven_test")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let run_gradle_compile = v
        .get("run_gradle_compile")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let run_gradle_test = v
        .get("run_gradle_test")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let run_docker_compose_ps = v
        .get("run_docker_compose_ps")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let run_podman_images = v
        .get("run_podman_images")
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
        && !run_ruff_check
        && !run_pytest
        && !run_mypy
        && !run_maven_compile
        && !run_maven_test
        && !run_gradle_compile
        && !run_gradle_test
        && !run_docker_compose_ps
        && !run_podman_images
    {
        return "错误：至少启用一项检查（含 run_cargo_* / run_frontend_* / run_python_* / run_maven_* / run_gradle_* / run_docker_compose_ps / run_podman_images）".to_string();
    }

    let mut sections: Vec<String> = Vec::new();
    let mut summary: Vec<(&'static str, &'static str)> = Vec::new();

    if run_cargo_fmt_check {
        let r = ci_tools::cargo_fmt_check_tool("{}", workspace_root, max_output_len);
        let failed = section_failed(&r);
        summary.push((
            "cargo fmt --check",
            if failed { "failed" } else { "passed" },
        ));
        sections.push(r);
        if fail_fast && failed {
            push_skipped(
                &mut summary,
                run_cargo_clippy,
                run_cargo_test,
                run_frontend_lint,
                run_frontend_prettier_check,
            );
            skip_python_steps(&mut summary, run_ruff_check, run_pytest, run_mypy);
            skip_jvm_container_steps(
                &mut summary,
                run_maven_compile,
                run_maven_test,
                run_gradle_compile,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
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
            skip_python_steps(&mut summary, run_ruff_check, run_pytest, run_mypy);
            skip_jvm_container_steps(
                &mut summary,
                run_maven_compile,
                run_maven_test,
                run_gradle_compile,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("cargo clippy", "skipped"));
    }

    if run_cargo_test {
        let r = if workspace_root.join("Cargo.toml").is_file() {
            cargo_tools::cargo_test("{}", workspace_root, max_output_len, None)
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
            skip_python_steps(&mut summary, run_ruff_check, run_pytest, run_mypy);
            skip_jvm_container_steps(
                &mut summary,
                run_maven_compile,
                run_maven_test,
                run_gradle_compile,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
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
            push_skipped(
                &mut summary,
                false,
                false,
                false,
                run_frontend_prettier_check,
            );
            skip_python_steps(&mut summary, run_ruff_check, run_pytest, run_mypy);
            skip_jvm_container_steps(
                &mut summary,
                run_maven_compile,
                run_maven_test,
                run_gradle_compile,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("frontend lint", "skipped"));
    }

    if run_frontend_prettier_check {
        let r = frontend_tools::frontend_prettier_check(
            r#"{"subdir":"frontend"}"#,
            workspace_root,
            max_output_len,
        );
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push((
            "frontend prettier --check",
            if failed { "failed" } else { "passed" },
        ));
        sections.push(r);
        if fail_fast && failed {
            skip_python_steps(&mut summary, run_ruff_check, run_pytest, run_mypy);
            skip_jvm_container_steps(
                &mut summary,
                run_maven_compile,
                run_maven_test,
                run_gradle_compile,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("frontend prettier --check", "skipped"));
    }

    if run_ruff_check {
        let r = python_tools::ruff_check("{}", workspace_root, max_output_len);
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push(("ruff check", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            if run_pytest {
                summary.push(("pytest", "skipped"));
            }
            if run_mypy {
                summary.push(("mypy", "skipped"));
            }
            skip_jvm_container_steps(
                &mut summary,
                run_maven_compile,
                run_maven_test,
                run_gradle_compile,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("ruff check", "skipped"));
    }

    if run_pytest {
        let r = python_tools::pytest_run("{}", workspace_root, max_output_len);
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push(("pytest", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            if run_mypy {
                summary.push(("mypy", "skipped"));
            }
            skip_jvm_container_steps(
                &mut summary,
                run_maven_compile,
                run_maven_test,
                run_gradle_compile,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("pytest", "skipped"));
    }

    if run_mypy {
        let r = python_tools::mypy_check("{}", workspace_root, max_output_len);
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push(("mypy", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            skip_jvm_container_steps(
                &mut summary,
                run_maven_compile,
                run_maven_test,
                run_gradle_compile,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("mypy", "skipped"));
    }

    if run_maven_compile {
        let r = jvm_tools::maven_compile("{}", workspace_root, max_output_len);
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push(("maven compile", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            skip_jvm_container_tail(
                &mut summary,
                run_maven_test,
                run_gradle_compile,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("maven compile", "skipped"));
    }

    if run_maven_test {
        let r = jvm_tools::maven_test("{}", workspace_root, max_output_len);
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push(("maven test", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            skip_jvm_container_tail(
                &mut summary,
                false,
                run_gradle_compile,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("maven test", "skipped"));
    }

    if run_gradle_compile {
        let r = jvm_tools::gradle_compile("{}", workspace_root, max_output_len);
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push(("gradle compile", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            skip_gradle_test_docker(
                &mut summary,
                run_gradle_test,
                run_docker_compose_ps,
                run_podman_images,
            );
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("gradle compile", "skipped"));
    }

    if run_gradle_test {
        let r = jvm_tools::gradle_test("{}", workspace_root, max_output_len);
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push(("gradle test", if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            skip_docker_podman_only(&mut summary, run_docker_compose_ps, run_podman_images);
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("gradle test", "skipped"));
    }

    if run_docker_compose_ps {
        let r = container_tools::docker_compose_ps("{}", workspace_root, max_output_len);
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push((
            "docker compose ps",
            if failed { "failed" } else { "passed" },
        ));
        sections.push(r);
        if fail_fast && failed && run_podman_images {
            summary.push(("podman images", "skipped"));
            return build_output(&summary, &sections, summary_only);
        }
    } else {
        summary.push(("docker compose ps", "skipped"));
    }

    if run_podman_images {
        let r = container_tools::podman_images("{}", workspace_root, max_output_len);
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push(("podman images", if failed { "failed" } else { "passed" }));
        sections.push(r);
    } else {
        summary.push(("podman images", "skipped"));
    }

    build_output(&summary, &sections, summary_only)
}

fn skip_jvm_container_steps(
    summary: &mut Vec<(&'static str, &'static str)>,
    maven_compile: bool,
    maven_test: bool,
    gradle_compile: bool,
    gradle_test: bool,
    docker_ps: bool,
    podman_img: bool,
) {
    if maven_compile {
        summary.push(("maven compile", "skipped"));
    }
    if maven_test {
        summary.push(("maven test", "skipped"));
    }
    if gradle_compile {
        summary.push(("gradle compile", "skipped"));
    }
    if gradle_test {
        summary.push(("gradle test", "skipped"));
    }
    if docker_ps {
        summary.push(("docker compose ps", "skipped"));
    }
    if podman_img {
        summary.push(("podman images", "skipped"));
    }
}

fn skip_jvm_container_tail(
    summary: &mut Vec<(&'static str, &'static str)>,
    maven_test: bool,
    gradle_compile: bool,
    gradle_test: bool,
    docker_ps: bool,
    podman_img: bool,
) {
    skip_jvm_container_steps(
        summary,
        false,
        maven_test,
        gradle_compile,
        gradle_test,
        docker_ps,
        podman_img,
    );
}

fn skip_gradle_test_docker(
    summary: &mut Vec<(&'static str, &'static str)>,
    gradle_test: bool,
    docker_ps: bool,
    podman_img: bool,
) {
    skip_jvm_container_steps(
        summary,
        false,
        false,
        false,
        gradle_test,
        docker_ps,
        podman_img,
    );
}

fn skip_docker_podman_only(
    summary: &mut Vec<(&'static str, &'static str)>,
    docker_ps: bool,
    podman_img: bool,
) {
    skip_jvm_container_steps(summary, false, false, false, false, docker_ps, podman_img);
}

fn section_failed(text: &str) -> bool {
    if text.contains("执行失败（") || text.contains(": 执行失败（") {
        return true;
    }
    if let Some(line) = text.lines().next() {
        if let Some(rest) = line.strip_prefix("cargo fmt --check (exit=") {
            return parse_exit_nonzero(rest);
        }
        if line.starts_with("cargo ")
            && line.contains("(exit=")
            && let Some(idx) = line.find("(exit=")
        {
            return parse_exit_nonzero(&line[idx..]);
        }
        if line.contains("npm run")
            && line.contains("(exit=")
            && let Some(idx) = line.find("(exit=")
        {
            return parse_exit_nonzero(&line[idx..]);
        }
        if line.contains("npx prettier")
            && line.contains("(exit=")
            && let Some(idx) = line.find("(exit=")
        {
            return parse_exit_nonzero(&line[idx..]);
        }
        if (line.starts_with("ruff check")
            || line.starts_with("python3 -m pytest")
            || line.starts_with("mypy"))
            && line.contains("(exit=")
            && let Some(idx) = line.find("(exit=")
        {
            return parse_exit_nonzero(&line[idx..]);
        }
        if (line.starts_with("mvn ")
            || line.starts_with("gradle ")
            || line.contains("docker compose ps")
            || line.starts_with("podman images"))
            && line.contains("(exit=")
            && let Some(idx) = line.find("(exit=")
        {
            return parse_exit_nonzero(&line[idx..]);
        }
    }
    text.contains("error: could not compile") || text.contains("error[E")
}

fn parse_exit_nonzero(s: &str) -> bool {
    // s 形如 "(exit=1):"、"1):"（fmt 首行 strip 后）等
    let s = s.trim();
    let rest = s.strip_prefix("(exit=").unwrap_or(s);
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

fn skip_python_steps(
    summary: &mut Vec<(&'static str, &'static str)>,
    ruff: bool,
    pytest: bool,
    mypy: bool,
) {
    if ruff {
        summary.push(("ruff check", "skipped"));
    }
    if pytest {
        summary.push(("pytest", "skipped"));
    }
    if mypy {
        summary.push(("mypy", "skipped"));
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

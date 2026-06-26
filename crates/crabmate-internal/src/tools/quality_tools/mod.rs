//! 质量与一致性：组合多项检查（Rust / 前端 / 可选 Python），便于 agent 一键拉齐。

use std::path::Path;

use super::tool_param_types::QualityWorkspaceArgs;
use super::{cargo_tools, ci_tools, container_tools, frontend_tools, jvm_tools, python_tools};

mod skip_tail;

#[derive(Clone, Copy)]
struct QualityFlags {
    run_cargo_fmt_check: bool,
    run_cargo_check: bool,
    run_cargo_clippy: bool,
    run_cargo_test: bool,
    run_frontend_lint: bool,
    run_frontend_build: bool,
    run_frontend_prettier_check: bool,
    run_ruff_check: bool,
    run_pytest: bool,
    run_mypy: bool,
    run_maven_compile: bool,
    run_maven_test: bool,
    run_gradle_compile: bool,
    run_gradle_test: bool,
    run_docker_compose_ps: bool,
    run_podman_images: bool,
    fail_fast: bool,
    summary_only: bool,
}

impl QualityFlags {
    fn any_step_enabled(self) -> bool {
        QualityStep::ORDER
            .into_iter()
            .any(|step| step.enabled(self))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QualityStep {
    CargoFmtCheck,
    CargoCheck,
    CargoClippy,
    CargoTest,
    FrontendLint,
    FrontendBuild,
    FrontendPrettierCheck,
    RuffCheck,
    Pytest,
    Mypy,
    MavenCompile,
    MavenTest,
    GradleCompile,
    GradleTest,
    DockerComposePs,
    PodmanImages,
}

impl QualityStep {
    const ORDER: [Self; 16] = [
        Self::CargoFmtCheck,
        Self::CargoCheck,
        Self::CargoClippy,
        Self::CargoTest,
        Self::FrontendLint,
        Self::FrontendBuild,
        Self::FrontendPrettierCheck,
        Self::RuffCheck,
        Self::Pytest,
        Self::Mypy,
        Self::MavenCompile,
        Self::MavenTest,
        Self::GradleCompile,
        Self::GradleTest,
        Self::DockerComposePs,
        Self::PodmanImages,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::CargoFmtCheck => "cargo fmt --check",
            Self::CargoCheck => "cargo check",
            Self::CargoClippy => "cargo clippy",
            Self::CargoTest => "cargo test",
            Self::FrontendLint => "frontend lint",
            Self::FrontendBuild => "frontend build",
            Self::FrontendPrettierCheck => "frontend prettier --check",
            Self::RuffCheck => "ruff check",
            Self::Pytest => "pytest",
            Self::Mypy => "mypy",
            Self::MavenCompile => "maven compile",
            Self::MavenTest => "maven test",
            Self::GradleCompile => "gradle compile",
            Self::GradleTest => "gradle test",
            Self::DockerComposePs => "docker compose ps",
            Self::PodmanImages => "podman images",
        }
    }

    fn enabled(self, f: QualityFlags) -> bool {
        match self {
            Self::CargoFmtCheck => f.run_cargo_fmt_check,
            Self::CargoCheck => f.run_cargo_check,
            Self::CargoClippy => f.run_cargo_clippy,
            Self::CargoTest => f.run_cargo_test,
            Self::FrontendLint => f.run_frontend_lint,
            Self::FrontendBuild => f.run_frontend_build,
            Self::FrontendPrettierCheck => f.run_frontend_prettier_check,
            Self::RuffCheck => f.run_ruff_check,
            Self::Pytest => f.run_pytest,
            Self::Mypy => f.run_mypy,
            Self::MavenCompile => f.run_maven_compile,
            Self::MavenTest => f.run_maven_test,
            Self::GradleCompile => f.run_gradle_compile,
            Self::GradleTest => f.run_gradle_test,
            Self::DockerComposePs => f.run_docker_compose_ps,
            Self::PodmanImages => f.run_podman_images,
        }
    }

    fn ignore_skip_marker(self) -> bool {
        matches!(
            self,
            Self::FrontendLint
                | Self::FrontendBuild
                | Self::FrontendPrettierCheck
                | Self::RuffCheck
                | Self::Pytest
                | Self::Mypy
                | Self::MavenCompile
                | Self::MavenTest
                | Self::GradleCompile
                | Self::GradleTest
                | Self::DockerComposePs
                | Self::PodmanImages
        )
    }

    fn skip_tail_after_failure(
        self,
        f: QualityFlags,
        summary: &mut Vec<(&'static str, &'static str)>,
    ) {
        skip_tail::skip_tail_after_failure(self, f, summary);
    }
}

fn run_quality_step(step: QualityStep, workspace_root: &Path, max_output_len: usize) -> String {
    let has_cargo = workspace_root.join("Cargo.toml").is_file();
    match step {
        QualityStep::CargoFmtCheck => {
            ci_tools::cargo_fmt_check_tool("{}", workspace_root, max_output_len)
        }
        QualityStep::CargoCheck => {
            if has_cargo {
                cargo_tools::cargo_check(r#"{"all_targets":true}"#, workspace_root, max_output_len)
            } else {
                "cargo check: 跳过（未找到 Cargo.toml）".to_string()
            }
        }
        QualityStep::CargoClippy => {
            if has_cargo {
                cargo_tools::cargo_clippy(r#"{"all_targets":true}"#, workspace_root, max_output_len)
            } else {
                "cargo clippy: 跳过（未找到 Cargo.toml）".to_string()
            }
        }
        QualityStep::CargoTest => {
            if has_cargo {
                cargo_tools::cargo_test("{}", workspace_root, max_output_len, None)
            } else {
                "cargo test: 跳过（未找到 Cargo.toml）".to_string()
            }
        }
        QualityStep::FrontendLint => {
            frontend_tools::frontend_lint(r#"{"script":"lint"}"#, workspace_root, max_output_len)
        }
        QualityStep::FrontendBuild => {
            frontend_tools::frontend_build(r#"{"script":"build"}"#, workspace_root, max_output_len)
        }
        QualityStep::FrontendPrettierCheck => {
            frontend_tools::frontend_prettier_check("{}", workspace_root, max_output_len)
        }
        QualityStep::RuffCheck => python_tools::ruff_check("{}", workspace_root, max_output_len),
        QualityStep::Pytest => python_tools::pytest_run("{}", workspace_root, max_output_len),
        QualityStep::Mypy => python_tools::mypy_check("{}", workspace_root, max_output_len),
        QualityStep::MavenCompile => jvm_tools::maven_compile("{}", workspace_root, max_output_len),
        QualityStep::MavenTest => jvm_tools::maven_test("{}", workspace_root, max_output_len),
        QualityStep::GradleCompile => {
            jvm_tools::gradle_compile("{}", workspace_root, max_output_len)
        }
        QualityStep::GradleTest => jvm_tools::gradle_test("{}", workspace_root, max_output_len),
        QualityStep::DockerComposePs => {
            container_tools::docker_compose_ps("{}", workspace_root, max_output_len)
        }
        QualityStep::PodmanImages => {
            container_tools::podman_images("{}", workspace_root, max_output_len)
        }
    }
}

fn step_failed(step: QualityStep, output: &str) -> bool {
    if step.ignore_skip_marker() && output.contains("跳过（") {
        return false;
    }
    section_failed(output)
}

/// 按开关组合运行多项质量检查；默认仅 Rust 侧 fmt + clippy（轻量）。
pub fn quality_workspace(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let a: QualityWorkspaceArgs = match serde_json::from_value(v) {
        Ok(x) => x,
        Err(e) => return format!("参数 JSON 与 quality_workspace 形状不一致: {e}"),
    };

    let flags = QualityFlags {
        run_cargo_fmt_check: a.run_cargo_fmt_check,
        run_cargo_check: a.run_cargo_check,
        run_cargo_clippy: a.run_cargo_clippy,
        run_cargo_test: a.run_cargo_test,
        run_frontend_lint: a.run_frontend_lint,
        run_frontend_build: a.run_frontend_build,
        run_frontend_prettier_check: a.run_frontend_prettier_check,
        run_ruff_check: a.run_ruff_check,
        run_pytest: a.run_pytest,
        run_mypy: a.run_mypy,
        run_maven_compile: a.run_maven_compile,
        run_maven_test: a.run_maven_test,
        run_gradle_compile: a.run_gradle_compile,
        run_gradle_test: a.run_gradle_test,
        run_docker_compose_ps: a.run_docker_compose_ps,
        run_podman_images: a.run_podman_images,
        fail_fast: a.fail_fast,
        summary_only: a.summary_only,
    };

    if !flags.any_step_enabled() {
        return "错误：至少启用一项检查（含 run_cargo_* / run_frontend_* / run_python_* / run_maven_* / run_gradle_* / run_docker_compose_ps / run_podman_images）".to_string();
    }

    let mut sections: Vec<String> = Vec::new();
    let mut summary: Vec<(&'static str, &'static str)> = Vec::new();

    for step in QualityStep::ORDER {
        if !step.enabled(flags) {
            summary.push((step.label(), "skipped"));
            continue;
        }
        let r = run_quality_step(step, workspace_root, max_output_len);
        let failed = step_failed(step, &r);
        summary.push((step.label(), if failed { "failed" } else { "passed" }));
        sections.push(r);
        if flags.fail_fast && failed {
            step.skip_tail_after_failure(flags, &mut summary);
            return build_output(&summary, &sections, flags.summary_only);
        }
    }

    build_output(&summary, &sections, flags.summary_only)
}

fn section_failed(text: &str) -> bool {
    section_failed_execution_markers(text)
        || section_failed_first_line_exit_patterns(text)
        || text.contains("error: could not compile")
        || text.contains("error[E")
}

fn section_failed_execution_markers(text: &str) -> bool {
    text.contains("执行失败（") || text.contains(": 执行失败（")
}

fn section_failed_first_line_exit_patterns(text: &str) -> bool {
    let Some(line) = text.lines().next() else {
        return false;
    };
    section_failed_cargo_fmt_exit(line)
        || section_failed_cargo_or_rustc_exit(line)
        || section_failed_npm_run_exit(line)
        || section_failed_npx_prettier_exit(line)
        || section_failed_python_linters_exit(line)
        || section_failed_jvm_docker_exit(line)
}

fn exit_suffix_nonzero(line: &str) -> bool {
    line.find("(exit=")
        .is_some_and(|idx| parse_exit_nonzero(&line[idx..]))
}

fn section_failed_cargo_fmt_exit(line: &str) -> bool {
    line.strip_prefix("cargo fmt --check (exit=")
        .is_some_and(parse_exit_nonzero)
}

fn section_failed_cargo_or_rustc_exit(line: &str) -> bool {
    (line.starts_with("cargo ") || line.starts_with("rustc ")) && exit_suffix_nonzero(line)
}

fn section_failed_npm_run_exit(line: &str) -> bool {
    line.contains("npm run") && exit_suffix_nonzero(line)
}

fn section_failed_npx_prettier_exit(line: &str) -> bool {
    line.contains("npx prettier") && exit_suffix_nonzero(line)
}

fn section_failed_python_linters_exit(line: &str) -> bool {
    (line.starts_with("ruff check")
        || line.starts_with("python3 -m pytest")
        || line.starts_with("mypy"))
        && exit_suffix_nonzero(line)
}

fn section_failed_jvm_docker_exit(line: &str) -> bool {
    (line.starts_with("mvn ")
        || line.starts_with("gradle ")
        || line.contains("docker compose ps")
        || line.starts_with("podman images"))
        && exit_suffix_nonzero(line)
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

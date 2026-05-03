//! 质量与一致性：组合多项检查（Rust / 前端 / 可选 Python），便于 agent 一键拉齐。

use std::path::Path;

use super::{cargo_tools, ci_tools, container_tools, frontend_tools, jvm_tools, python_tools};

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
        self.run_cargo_fmt_check
            || self.run_cargo_check
            || self.run_cargo_clippy
            || self.run_cargo_test
            || self.run_frontend_lint
            || self.run_frontend_build
            || self.run_frontend_prettier_check
            || self.run_ruff_check
            || self.run_pytest
            || self.run_mypy
            || self.run_maven_compile
            || self.run_maven_test
            || self.run_gradle_compile
            || self.run_gradle_test
            || self.run_docker_compose_ps
            || self.run_podman_images
    }
}

#[derive(Clone, Copy)]
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
        match self {
            Self::CargoFmtCheck => {
                push_skipped(
                    summary,
                    f.run_cargo_check,
                    f.run_cargo_clippy,
                    f.run_cargo_test,
                    f.run_frontend_lint,
                    f.run_frontend_build,
                    f.run_frontend_prettier_check,
                );
                skip_python_steps(summary, f.run_ruff_check, f.run_pytest, f.run_mypy);
                skip_jvm_container_steps(
                    summary,
                    f.run_maven_compile,
                    f.run_maven_test,
                    f.run_gradle_compile,
                    f.run_gradle_test,
                    f.run_docker_compose_ps,
                    f.run_podman_images,
                );
            }
            Self::CargoCheck => {
                push_skipped(
                    summary,
                    false,
                    f.run_cargo_clippy,
                    f.run_cargo_test,
                    f.run_frontend_lint,
                    f.run_frontend_build,
                    f.run_frontend_prettier_check,
                );
                skip_python_steps(summary, f.run_ruff_check, f.run_pytest, f.run_mypy);
                skip_jvm_container_steps(
                    summary,
                    f.run_maven_compile,
                    f.run_maven_test,
                    f.run_gradle_compile,
                    f.run_gradle_test,
                    f.run_docker_compose_ps,
                    f.run_podman_images,
                );
            }
            Self::CargoClippy => {
                push_skipped(
                    summary,
                    false,
                    false,
                    f.run_cargo_test,
                    f.run_frontend_lint,
                    f.run_frontend_build,
                    f.run_frontend_prettier_check,
                );
                skip_python_steps(summary, f.run_ruff_check, f.run_pytest, f.run_mypy);
                skip_jvm_container_steps(
                    summary,
                    f.run_maven_compile,
                    f.run_maven_test,
                    f.run_gradle_compile,
                    f.run_gradle_test,
                    f.run_docker_compose_ps,
                    f.run_podman_images,
                );
            }
            Self::CargoTest => {
                push_skipped(
                    summary,
                    false,
                    false,
                    false,
                    f.run_frontend_lint,
                    f.run_frontend_build,
                    f.run_frontend_prettier_check,
                );
                skip_python_steps(summary, f.run_ruff_check, f.run_pytest, f.run_mypy);
                skip_jvm_container_steps(
                    summary,
                    f.run_maven_compile,
                    f.run_maven_test,
                    f.run_gradle_compile,
                    f.run_gradle_test,
                    f.run_docker_compose_ps,
                    f.run_podman_images,
                );
            }
            Self::FrontendLint => {
                push_skipped(
                    summary,
                    false,
                    false,
                    false,
                    false,
                    f.run_frontend_build,
                    f.run_frontend_prettier_check,
                );
                skip_python_steps(summary, f.run_ruff_check, f.run_pytest, f.run_mypy);
                skip_jvm_container_steps(
                    summary,
                    f.run_maven_compile,
                    f.run_maven_test,
                    f.run_gradle_compile,
                    f.run_gradle_test,
                    f.run_docker_compose_ps,
                    f.run_podman_images,
                );
            }
            Self::FrontendBuild => {
                push_skipped(
                    summary,
                    false,
                    false,
                    false,
                    false,
                    false,
                    f.run_frontend_prettier_check,
                );
                skip_python_steps(summary, f.run_ruff_check, f.run_pytest, f.run_mypy);
                skip_jvm_container_steps(
                    summary,
                    f.run_maven_compile,
                    f.run_maven_test,
                    f.run_gradle_compile,
                    f.run_gradle_test,
                    f.run_docker_compose_ps,
                    f.run_podman_images,
                );
            }
            Self::FrontendPrettierCheck => {
                skip_python_steps(summary, f.run_ruff_check, f.run_pytest, f.run_mypy);
                skip_jvm_container_steps(
                    summary,
                    f.run_maven_compile,
                    f.run_maven_test,
                    f.run_gradle_compile,
                    f.run_gradle_test,
                    f.run_docker_compose_ps,
                    f.run_podman_images,
                );
            }
            Self::RuffCheck => {
                if f.run_pytest {
                    summary.push(("pytest", "skipped"));
                }
                if f.run_mypy {
                    summary.push(("mypy", "skipped"));
                }
                skip_jvm_container_steps(
                    summary,
                    f.run_maven_compile,
                    f.run_maven_test,
                    f.run_gradle_compile,
                    f.run_gradle_test,
                    f.run_docker_compose_ps,
                    f.run_podman_images,
                );
            }
            Self::Pytest => {
                if f.run_mypy {
                    summary.push(("mypy", "skipped"));
                }
                skip_jvm_container_steps(
                    summary,
                    f.run_maven_compile,
                    f.run_maven_test,
                    f.run_gradle_compile,
                    f.run_gradle_test,
                    f.run_docker_compose_ps,
                    f.run_podman_images,
                );
            }
            Self::Mypy => skip_jvm_container_steps(
                summary,
                f.run_maven_compile,
                f.run_maven_test,
                f.run_gradle_compile,
                f.run_gradle_test,
                f.run_docker_compose_ps,
                f.run_podman_images,
            ),
            Self::MavenCompile => skip_jvm_container_tail(
                summary,
                f.run_maven_test,
                f.run_gradle_compile,
                f.run_gradle_test,
                f.run_docker_compose_ps,
                f.run_podman_images,
            ),
            Self::MavenTest => skip_jvm_container_tail(
                summary,
                false,
                f.run_gradle_compile,
                f.run_gradle_test,
                f.run_docker_compose_ps,
                f.run_podman_images,
            ),
            Self::GradleCompile => skip_gradle_test_docker(
                summary,
                f.run_gradle_test,
                f.run_docker_compose_ps,
                f.run_podman_images,
            ),
            Self::GradleTest => {
                skip_docker_podman_only(summary, f.run_docker_compose_ps, f.run_podman_images)
            }
            Self::DockerComposePs => {
                if f.run_podman_images {
                    summary.push(("podman images", "skipped"));
                }
            }
            Self::PodmanImages => {}
        }
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

    let flags = QualityFlags {
        run_cargo_fmt_check: v
            .get("run_cargo_fmt_check")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        run_cargo_check: v
            .get("run_cargo_check")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_cargo_clippy: v
            .get("run_cargo_clippy")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        run_cargo_test: v
            .get("run_cargo_test")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_frontend_lint: v
            .get("run_frontend_lint")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_frontend_build: v
            .get("run_frontend_build")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_frontend_prettier_check: v
            .get("run_frontend_prettier_check")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_ruff_check: v
            .get("run_ruff_check")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_pytest: v
            .get("run_pytest")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_mypy: v.get("run_mypy").and_then(|x| x.as_bool()).unwrap_or(false),
        run_maven_compile: v
            .get("run_maven_compile")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_maven_test: v
            .get("run_maven_test")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_gradle_compile: v
            .get("run_gradle_compile")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_gradle_test: v
            .get("run_gradle_test")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_docker_compose_ps: v
            .get("run_docker_compose_ps")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        run_podman_images: v
            .get("run_podman_images")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        fail_fast: v.get("fail_fast").and_then(|x| x.as_bool()).unwrap_or(true),
        summary_only: v
            .get("summary_only")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
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

fn push_skipped(
    summary: &mut Vec<(&'static str, &'static str)>,
    cargo_check: bool,
    clippy: bool,
    test: bool,
    fe_lint: bool,
    fe_build: bool,
    fe_fmt: bool,
) {
    if cargo_check {
        summary.push(("cargo check", "skipped"));
    }
    if clippy {
        summary.push(("cargo clippy", "skipped"));
    }
    if test {
        summary.push(("cargo test", "skipped"));
    }
    if fe_lint {
        summary.push(("frontend lint", "skipped"));
    }
    if fe_build {
        summary.push(("frontend build", "skipped"));
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

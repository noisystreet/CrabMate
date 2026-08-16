use super::{QualityFlags, QualityStep};

/// `fail_fast` 时：将当前步骤之后仍应运行但被跳过的步骤写入汇总（与 [`QualityStep::ORDER`] 对齐）。
pub(super) fn skip_tail_after_failure(
    step: QualityStep,
    f: QualityFlags,
    summary: &mut Vec<(&'static str, &'static str)>,
) {
    let Some(failed_idx) = QualityStep::ORDER.iter().position(|&s| s == step) else {
        return;
    };
    for &remaining in &QualityStep::ORDER[failed_idx + 1..] {
        if remaining.enabled(f) {
            summary.push((remaining.label(), "skipped"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags_cargo_chain() -> QualityFlags {
        QualityFlags {
            run_cargo_fmt_check: true,
            run_cargo_check: true,
            run_cargo_clippy: true,
            run_cargo_test: false,
            run_frontend_lint: false,
            run_frontend_build: false,
            run_frontend_prettier_check: false,
            run_ruff_check: false,
            run_pytest: false,
            run_mypy: false,
            run_maven_compile: false,
            run_maven_test: false,
            run_gradle_compile: false,
            run_gradle_test: false,
            run_docker_compose_ps: false,
            run_podman_images: false,
            fail_fast: true,
            summary_only: false,
        }
    }

    #[test]
    fn skip_tail_after_cargo_fmt_marks_later_enabled_steps() {
        let mut summary = Vec::new();
        skip_tail_after_failure(
            QualityStep::CargoFmtCheck,
            flags_cargo_chain(),
            &mut summary,
        );
        let names: Vec<_> = summary.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"cargo check"));
        assert!(names.contains(&"cargo clippy"));
        assert!(!names.contains(&"cargo fmt --check"));
    }

    #[test]
    fn skip_tail_after_gradle_test_only_docker_tail() {
        let f = QualityFlags {
            run_gradle_test: true,
            run_docker_compose_ps: true,
            run_podman_images: true,
            fail_fast: true,
            ..flags_cargo_chain()
        };
        let mut summary = Vec::new();
        skip_tail_after_failure(QualityStep::GradleTest, f, &mut summary);
        let names: Vec<_> = summary.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["docker compose ps", "podman images"]);
    }
}

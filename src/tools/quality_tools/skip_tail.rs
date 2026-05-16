use super::{
    QualityFlags, QualityStep, push_skipped, skip_docker_podman_only, skip_gradle_test_docker,
    skip_jvm_container_steps, skip_jvm_container_tail, skip_python_steps,
};

/// `fail_fast` 时：将当前步骤之后仍应运行但被跳过的步骤写入汇总（与 [`QualityStep::ORDER`] 对齐）。
pub(super) fn skip_tail_after_failure(
    step: QualityStep,
    f: QualityFlags,
    summary: &mut Vec<(&'static str, &'static str)>,
) {
    match step {
        QualityStep::CargoFmtCheck => {
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
        QualityStep::CargoCheck => {
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
        QualityStep::CargoClippy => {
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
        QualityStep::CargoTest => {
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
        QualityStep::FrontendLint => {
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
        QualityStep::FrontendBuild => {
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
        QualityStep::FrontendPrettierCheck => {
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
        QualityStep::RuffCheck => {
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
        QualityStep::Pytest => {
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
        QualityStep::Mypy => skip_jvm_container_steps(
            summary,
            f.run_maven_compile,
            f.run_maven_test,
            f.run_gradle_compile,
            f.run_gradle_test,
            f.run_docker_compose_ps,
            f.run_podman_images,
        ),
        QualityStep::MavenCompile => skip_jvm_container_tail(
            summary,
            f.run_maven_test,
            f.run_gradle_compile,
            f.run_gradle_test,
            f.run_docker_compose_ps,
            f.run_podman_images,
        ),
        QualityStep::MavenTest => skip_jvm_container_tail(
            summary,
            false,
            f.run_gradle_compile,
            f.run_gradle_test,
            f.run_docker_compose_ps,
            f.run_podman_images,
        ),
        QualityStep::GradleCompile => skip_gradle_test_docker(
            summary,
            f.run_gradle_test,
            f.run_docker_compose_ps,
            f.run_podman_images,
        ),
        QualityStep::GradleTest => {
            skip_docker_podman_only(summary, f.run_docker_compose_ps, f.run_podman_images)
        }
        QualityStep::DockerComposePs => {
            if f.run_podman_images {
                summary.push(("podman images", "skipped"));
            }
        }
        QualityStep::PodmanImages => {}
    }
}

//! 从 [`super::super::task::TaskResult`] 更新 [`super::super::build_state::BuildState`]（拆出以降低 `execution_impl` 圈复杂度）。

use super::super::build_state::BuildState;
use super::super::task::{ArtifactKind, BuildArtifactKind, TaskResult};

pub(super) fn apply_task_result_to_build_state(build_state: &mut BuildState, result: &TaskResult) {
    for artifact in &result.artifacts {
        apply_one_artifact(build_state, artifact);
    }
    hint_build_dir_from_task_output(build_state, result.output.as_deref());
}

fn apply_one_artifact(build_state: &mut BuildState, artifact: &super::super::task::Artifact) {
    match &artifact.kind {
        ArtifactKind::BuildArtifact(build_kind) => {
            let Some(ref path) = artifact.path else {
                return;
            };
            let path_buf = std::path::PathBuf::from(path);
            match build_kind {
                BuildArtifactKind::SourceFile => {
                    if let Some(ref content) = artifact.content {
                        build_state.record_source_file(&path_buf, content);
                    }
                }
                BuildArtifactKind::ObjectFile => {
                    build_state.add_object_file(path_buf);
                }
                BuildArtifactKind::Executable => {
                    build_state.add_executable(path_buf);
                }
                BuildArtifactKind::StaticLibrary => {
                    build_state.add_static_library(path_buf);
                }
                BuildArtifactKind::DynamicLibrary => {
                    build_state.add_dynamic_library(path_buf);
                }
                BuildArtifactKind::BuildLog => {}
            }
        }
        ArtifactKind::File => {
            let Some(ref path) = artifact.path else {
                return;
            };
            let path_buf = std::path::PathBuf::from(path);
            if let Some(ext) = path_buf.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if matches!(ext.as_str(), "c" | "cpp" | "cc" | "h" | "hpp")
                    && let Some(ref content) = artifact.content
                {
                    build_state.record_source_file(&path_buf, content);
                }
            }
        }
        _ => {}
    }
}

fn hint_build_dir_from_task_output(build_state: &mut BuildState, output: Option<&str>) {
    let Some(output) = output else {
        return;
    };
    for line in output.lines() {
        if (line.contains("build/") || line.contains("Build directory:"))
            && let Some(idx) = line.find("build")
        {
            let build_dir = line[idx..].split_whitespace().next().unwrap_or("build");
            let build_path = std::path::PathBuf::from(build_dir);
            if build_path.exists() {
                build_state.set_build_dir(build_path);
                break;
            }
        }
    }
}

//! Development 工具子域标签：按语言栈 / 职责过滤，与 [`super::ToolCategory::Development`] 配合使用。

/// 与工作区、壳命令、编排、元数据等相关，**不绑定**特定语言运行时。
pub const GENERAL: &str = "general";
/// Rust / Cargo 生态工具。
pub const RUST: &str = "rust";
/// 前端（npm/Node）脚本类工具。
pub const FRONTEND: &str = "frontend";
/// Python（ruff/pytest/mypy/pip/uv 等）。
pub const PYTHON: &str = "python";
/// C / C++（clang-format、run_command 白名单中的编译/构建工具等）。
pub const CPP: &str = "cpp";
/// Git 版本控制。
pub const VCS: &str = "vcs";
/// 静态检查、审计、CI 聚合等质量类工具（常与 RUST/FRONTEND 重叠）。
pub const QUALITY: &str = "quality";
/// Go 语言工具链。
pub const GO: &str = "go";
/// JVM（Maven / Gradle）。
pub const JVM: &str = "jvm";
/// Shell 脚本相关工具。
pub const SHELL: &str = "shell";
/// Docker / 容器化相关工具。
pub const DOCKER: &str = "docker";

/// 返回 `Development` 工具的标签切片；**非 Development 工具名**返回空（调用方按分类跳过）。
///
/// #1157：标签的唯一真源是 [`super::ToolSpec::tags`]（声明点在
/// `tool_specs_registry/specs/*.inc.rs`）。此前这里另有一张约 200 行的 `match`，与声明点
/// 各写一份——漏改其一不会报错，只会在 [`super::build_tools_with_options`] 的栈裁剪里
/// **静默丢工具**（或留下本不该出现的工具）。
///
/// 未知名字返回 `&[]`：旧实现的 `_ => &[GENERAL]` 兜底只对「已注册但未列名」的
/// Development 工具生效，而那批工具现在已在声明点显式写了 `tags`；两个调用点
/// （`tool_passes_filters` 与 `builtin_tool_descriptors`）都只传已注册的 `spec.name`。
pub fn tags_for_tool_name(name: &str) -> &'static [&'static str] {
    super::tool_specs_registry::tool_specs()
        .iter()
        .find(|spec| spec.name == name)
        .map_or(&[], |spec| spec.tags)
}

/// 根据工作区根目录推测应启用的开发工具标签（去重）。始终包含 `general` 与 `vcs`。
pub fn suggest_dev_tags_for_workspace(root: &std::path::Path) -> Vec<&'static str> {
    let mut out = vec![GENERAL, VCS];
    suggest_tags_from_manifest_files(root, &mut out);
    if suggest_shell_tag_if_scripts(root) {
        out.push(SHELL);
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn suggest_tags_from_manifest_files(root: &std::path::Path, out: &mut Vec<&'static str>) {
    suggest_rust_tag_if_cargo_toml(root, out);
    suggest_frontend_tag_if_package_json(root, out);
    suggest_python_tag_if_python_manifest(root, out);
    suggest_cpp_tag_if_native_build_files(root, out);
    suggest_go_tag_if_go_mod(root, out);
    suggest_jvm_tag_if_gradle_or_maven(root, out);
    suggest_docker_tag_if_docker_files(root, out);
}

fn suggest_rust_tag_if_cargo_toml(root: &std::path::Path, out: &mut Vec<&'static str>) {
    if root.join("Cargo.toml").is_file() {
        out.push(RUST);
    }
}

fn suggest_frontend_tag_if_package_json(root: &std::path::Path, out: &mut Vec<&'static str>) {
    if root.join("frontend").join("package.json").is_file() || root.join("package.json").is_file() {
        out.push(FRONTEND);
    }
}

fn suggest_python_tag_if_python_manifest(root: &std::path::Path, out: &mut Vec<&'static str>) {
    if root.join("pyproject.toml").is_file()
        || root.join("setup.py").is_file()
        || root.join("setup.cfg").is_file()
        || root.join("requirements.txt").is_file()
    {
        out.push(PYTHON);
    }
}

fn suggest_cpp_tag_if_native_build_files(root: &std::path::Path, out: &mut Vec<&'static str>) {
    if root.join("CMakeLists.txt").is_file()
        || root.join("Makefile").is_file()
        || root.join("meson.build").is_file()
        || root.join("configure.ac").is_file()
        || root.join("configure.in").is_file()
    {
        out.push(CPP);
    }
}

fn suggest_go_tag_if_go_mod(root: &std::path::Path, out: &mut Vec<&'static str>) {
    if root.join("go.mod").is_file() {
        out.push(GO);
    }
}

fn suggest_jvm_tag_if_gradle_or_maven(root: &std::path::Path, out: &mut Vec<&'static str>) {
    if root.join("pom.xml").is_file()
        || root.join("build.gradle").is_file()
        || root.join("build.gradle.kts").is_file()
        || root.join("settings.gradle").is_file()
        || root.join("settings.gradle.kts").is_file()
    {
        out.push(JVM);
    }
}

fn suggest_docker_tag_if_docker_files(root: &std::path::Path, out: &mut Vec<&'static str>) {
    if root.join("Dockerfile").is_file()
        || root.join("docker-compose.yml").is_file()
        || root.join("docker-compose.yaml").is_file()
        || root.join(".dockerignore").is_file()
    {
        out.push(DOCKER);
    }
}

fn suggest_shell_tag_if_scripts(root: &std::path::Path) -> bool {
    root.join("scripts").is_dir()
        || std::fs::read_dir(root)
            .ok()
            .map(|entries| {
                entries.flatten().any(|e| {
                    e.path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| matches!(ext, "sh" | "bash"))
                })
            })
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_names as tool_names;

    /// #1157：标签真源搬到 `ToolSpec::tags` 后，`Development` 工具必须声明非空标签，
    /// `Basic` 工具必须为空——否则 `dev_tags` 栈裁剪会把 Basic 工具误算进/漏出。
    #[test]
    fn spec_tags_follow_category() {
        for spec in super::super::tool_specs_registry::tool_specs() {
            match spec.category {
                crate::cm_tools::tools::ToolCategory::Development => assert!(
                    !spec.tags.is_empty(),
                    "Development 工具 `{}` 未声明开发子域标签",
                    spec.name
                ),
                crate::cm_tools::tools::ToolCategory::Basic => assert!(
                    spec.tags.is_empty(),
                    "Basic 工具 `{}` 不应声明开发子域标签（得到 {:?}）",
                    spec.name,
                    spec.tags
                ),
            }
        }
    }

    /// 标签取值必须是本模块导出的常量，禁止在 spec 里手写 `"rust"` 之类的自由字符串
    /// （拼错不会有编译错误，只会让栈裁剪静默失效）。
    #[test]
    fn spec_tags_use_known_constants() {
        const KNOWN: &[&str] = &[
            GENERAL, RUST, FRONTEND, PYTHON, CPP, VCS, QUALITY, GO, JVM, SHELL, DOCKER,
        ];
        for spec in super::super::tool_specs_registry::tool_specs() {
            for tag in spec.tags {
                assert!(
                    KNOWN.contains(tag),
                    "工具 `{}` 的标签 `{tag}` 不在 dev_tag 常量集合中",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn workflow_execute_tags_rust_and_quality() {
        let t = tags_for_tool_name(tool_names::WORKFLOW_EXECUTE);
        assert!(t.contains(&RUST) && t.contains(&QUALITY) && t.contains(&GENERAL));
    }

    #[test]
    fn cargo_check_is_rust_and_quality() {
        let t = tags_for_tool_name(tool_names::CARGO_CHECK);
        assert!(t.contains(&RUST) && t.contains(&QUALITY));
    }

    #[test]
    fn git_is_vcs() {
        let t = tags_for_tool_name(tool_names::GIT);
        assert!(t.contains(&VCS));
    }

    #[test]
    fn suggest_tags_includes_python_when_pyproject() {
        let dir = std::env::temp_dir().join(format!("crabmate_dev_tag_py_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let s = suggest_dev_tags_for_workspace(&dir);
        assert!(s.contains(&PYTHON));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggest_tags_includes_rust_when_cargo_toml() {
        let dir =
            std::env::temp_dir().join(format!("crabmate_dev_tag_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        let s = suggest_dev_tags_for_workspace(&dir);
        assert!(s.contains(&RUST));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggest_tags_includes_cpp_when_configure_ac() {
        let dir = std::env::temp_dir().join(format!("crabmate_dev_tag_ac_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("configure.ac"), "AC_INIT([x],[1.0])\n").unwrap();
        let s = suggest_dev_tags_for_workspace(&dir);
        assert!(s.contains(&CPP));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggest_tags_includes_jvm_when_pom() {
        let dir = std::env::temp_dir().join(format!("crabmate_dev_tag_jvm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pom.xml"), "<project/>").unwrap();
        let s = suggest_dev_tags_for_workspace(&dir);
        assert!(s.contains(&JVM));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggest_tags_includes_cpp_when_cmake() {
        let dir = std::env::temp_dir().join(format!("crabmate_dev_tag_cpp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.16)\n",
        )
        .unwrap();
        let s = suggest_dev_tags_for_workspace(&dir);
        assert!(s.contains(&CPP));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

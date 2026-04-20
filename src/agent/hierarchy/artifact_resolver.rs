//! 产物解析器：自动发现前序步骤产物
//!
//! 用于在 Operator 执行前自动解析依赖的产物路径，
//! 支持按类型查找、按模式匹配、按构建状态解析等。

use std::path::PathBuf;

use super::artifact_store::ArtifactStore;
use super::build_state::BuildState;
use super::task::{Artifact, ArtifactKind, BuildArtifactKind};

/// 产物解析器
pub struct ArtifactResolver<'a> {
    artifact_store: &'a ArtifactStore,
    build_state: Option<&'a BuildState>,
}

impl<'a> ArtifactResolver<'a> {
    /// 创建产物解析器
    pub fn new(artifact_store: &'a ArtifactStore, build_state: Option<&'a BuildState>) -> Self {
        Self {
            artifact_store,
            build_state,
        }
    }

    /// 根据产物类型查找
    pub fn find_by_kind(&self, kind: ArtifactKind) -> Vec<&Artifact> {
        self.artifact_store
            .all()
            .into_iter()
            .filter(|a| a.kind == kind)
            .collect()
    }

    /// 根据构建产物类型查找
    pub fn find_by_build_kind(&self, kind: BuildArtifactKind) -> Vec<&Artifact> {
        self.artifact_store
            .all()
            .into_iter()
            .filter(|a| match &a.kind {
                ArtifactKind::BuildArtifact(k) => *k == kind,
                _ => false,
            })
            .collect()
    }

    /// 根据路径模式查找（支持通配符）
    pub fn find_by_pattern(&self, pattern: &str) -> Vec<&Artifact> {
        let pattern = pattern.to_lowercase();
        self.artifact_store
            .all()
            .into_iter()
            .filter(|a| {
                a.path.as_ref().is_some_and(|p| {
                    let path_lower = p.to_lowercase();
                    // 简单通配符匹配：*.cpp, main.* 等
                    if pattern.starts_with("*.") {
                        path_lower.ends_with(&pattern[1..])
                    } else if pattern.ends_with(".*") {
                        let prefix = &pattern[..pattern.len() - 2];
                        path_lower
                            .split('/')
                            .next_back()
                            .is_some_and(|name| name.starts_with(prefix))
                    } else {
                        path_lower.contains(&pattern)
                    }
                })
            })
            .collect()
    }

    /// 根据文件名查找
    pub fn find_by_name(&self, name: &str) -> Vec<&Artifact> {
        let name_lower = name.to_lowercase();
        self.artifact_store
            .all()
            .into_iter()
            .filter(|a| {
                a.name.to_lowercase() == name_lower
                    || a.path.as_ref().is_some_and(|p| {
                        // 从路径中提取文件名
                        let path = std::path::Path::new(p);
                        path.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.to_lowercase() == name_lower)
                    })
            })
            .collect()
    }

    /// 获取构建产物的完整路径
    pub fn resolve_build_artifact(&self, name: &str) -> Option<PathBuf> {
        // 首先尝试从 BuildState 查找
        if let Some(state) = self.build_state {
            // 尝试查找可执行文件
            if let Some(path) = state.find_executable(name) {
                return Some(path.clone());
            }
            // 尝试查找目标文件
            if let Some(path) = state.find_object_file(name) {
                return Some(path.clone());
            }
        }

        // 从 ArtifactStore 查找
        self.find_by_name(name)
            .into_iter()
            .filter(|a| matches!(a.kind, ArtifactKind::BuildArtifact(_)))
            .filter_map(|a| a.path.as_ref().map(PathBuf::from))
            .next()
    }

    /// 获取源码文件路径
    pub fn resolve_source_file(&self, name: &str) -> Option<PathBuf> {
        self.find_by_build_kind(BuildArtifactKind::SourceFile)
            .into_iter()
            .find(|a| {
                a.name == name
                    || a.path.as_ref().is_some_and(|p| {
                        // 从路径中提取文件 stem
                        let path = std::path::Path::new(p);
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .is_some_and(|s| s == name)
                    })
            })
            .and_then(|a| a.path.as_ref().map(PathBuf::from))
    }

    /// 获取所有源码文件
    pub fn get_all_source_files(&self) -> Vec<&Artifact> {
        self.find_by_build_kind(BuildArtifactKind::SourceFile)
    }

    /// 获取所有目标文件
    pub fn get_all_object_files(&self) -> Vec<&Artifact> {
        self.find_by_build_kind(BuildArtifactKind::ObjectFile)
    }

    /// 获取所有可执行文件
    pub fn get_all_executables(&self) -> Vec<&Artifact> {
        self.find_by_build_kind(BuildArtifactKind::Executable)
    }

    /// 为工具调用参数注入产物路径
    ///
    /// 将参数中的占位符（如 `{artifact:main.cpp}`）替换为实际路径
    pub fn inject_artifact_paths(&self, args: &mut [String]) {
        for arg in args.iter_mut() {
            if arg.starts_with("{artifact:") && arg.ends_with("}") {
                let name = &arg[10..arg.len() - 1];
                if let Some(path) = self.resolve_source_file(name) {
                    *arg = path.to_string_lossy().to_string();
                } else if let Some(path) = self.resolve_build_artifact(name) {
                    *arg = path.to_string_lossy().to_string();
                }
            }
        }
    }

    /// 根据构建需求获取所需产物
    pub fn resolve_build_requirements(
        &self,
        requirements: &[BuildArtifactKind],
    ) -> Vec<(BuildArtifactKind, Option<PathBuf>)> {
        requirements
            .iter()
            .map(|kind| {
                let path = match kind {
                    BuildArtifactKind::SourceFile => self
                        .get_all_source_files()
                        .first()
                        .and_then(|a| a.path.as_ref().map(PathBuf::from)),
                    BuildArtifactKind::ObjectFile => self
                        .get_all_object_files()
                        .first()
                        .and_then(|a| a.path.as_ref().map(PathBuf::from)),
                    BuildArtifactKind::Executable => self
                        .get_all_executables()
                        .first()
                        .and_then(|a| a.path.as_ref().map(PathBuf::from)),
                    _ => self
                        .find_by_build_kind(*kind)
                        .first()
                        .and_then(|a| a.path.as_ref().map(PathBuf::from)),
                };
                (*kind, path)
            })
            .collect()
    }

    /// 格式化产物列表供 LLM 使用
    pub fn format_for_llm(&self) -> String {
        let artifacts = self.artifact_store.all();
        if artifacts.is_empty() {
            return "无可用产物".to_string();
        }

        let mut lines = vec!["可用产物:".to_string()];
        for artifact in artifacts {
            let kind_str = match &artifact.kind {
                ArtifactKind::BuildArtifact(k) => format!("{:?}", k),
                _ => format!("{:?}", artifact.kind),
            };
            let path_str = artifact
                .path
                .clone()
                .unwrap_or_else(|| artifact.name.clone());
            lines.push(format!(
                "  - [{}] {}: {}",
                kind_str, artifact.name, path_str
            ));
        }
        lines.join("\n")
    }
}

/// 为工具执行准备环境变量
///
/// 设置与构建相关的环境变量，如源文件路径、构建目录等
pub fn prepare_build_env(
    artifact_store: &ArtifactStore,
    build_state: Option<&BuildState>,
) -> Vec<(String, String)> {
    let resolver = ArtifactResolver::new(artifact_store, build_state);
    let mut env = Vec::new();

    // 设置源文件路径
    let sources: Vec<_> = resolver
        .get_all_source_files()
        .iter()
        .filter_map(|a| a.path.clone())
        .collect();
    if !sources.is_empty() {
        env.push(("SOURCE_FILES".to_string(), sources.join(" ")));
    }

    // 设置目标文件路径
    let objects: Vec<_> = resolver
        .get_all_object_files()
        .iter()
        .filter_map(|a| a.path.clone())
        .collect();
    if !objects.is_empty() {
        env.push(("OBJECT_FILES".to_string(), objects.join(" ")));
    }

    // 设置可执行文件路径
    if let Some(path) = resolver
        .get_all_executables()
        .first()
        .and_then(|exe| exe.path.clone())
    {
        env.push(("EXECUTABLE".to_string(), path));
    }

    // 设置构建目录
    if let Some(dir) = build_state.and_then(|s| s.build_dir.clone()) {
        env.push(("BUILD_DIR".to_string(), dir.to_string_lossy().to_string()));
    }

    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_artifact(id: &str, name: &str, kind: ArtifactKind, path: &str) -> Artifact {
        Artifact {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            path: Some(path.to_string()),
            content: None,
            metadata: serde_json::Value::Null,
            produced_by: "test".to_string(),
            consumed_by: Vec::new(),
        }
    }

    #[test]
    fn test_find_by_build_kind() {
        let mut store = ArtifactStore::new();
        let artifact = create_test_artifact(
            "1",
            "main.cpp",
            ArtifactKind::BuildArtifact(BuildArtifactKind::SourceFile),
            "/src/main.cpp",
        );
        store.put(artifact);

        let resolver = ArtifactResolver::new(&store, None);
        let results = resolver.find_by_build_kind(BuildArtifactKind::SourceFile);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "main.cpp");
    }

    #[test]
    fn test_find_by_pattern() {
        let mut store = ArtifactStore::new();
        store.put(create_test_artifact(
            "1",
            "main.cpp",
            ArtifactKind::BuildArtifact(BuildArtifactKind::SourceFile),
            "/src/main.cpp",
        ));
        store.put(create_test_artifact(
            "2",
            "lib.cpp",
            ArtifactKind::BuildArtifact(BuildArtifactKind::SourceFile),
            "/src/lib.cpp",
        ));

        let resolver = ArtifactResolver::new(&store, None);
        let results = resolver.find_by_pattern("*.cpp");

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_inject_artifact_paths() {
        let mut store = ArtifactStore::new();
        store.put(create_test_artifact(
            "1",
            "main.cpp",
            ArtifactKind::BuildArtifact(BuildArtifactKind::SourceFile),
            "/src/main.cpp",
        ));

        let resolver = ArtifactResolver::new(&store, None);
        let mut args = vec![
            "g++".to_string(),
            "{artifact:main.cpp}".to_string(),
            "-o".to_string(),
            "main".to_string(),
        ];
        resolver.inject_artifact_paths(&mut args);

        assert_eq!(args[1], "/src/main.cpp");
    }
}

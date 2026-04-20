//! 构建状态管理（编译任务专用）
//!
//! 用于追踪 C++/Rust 等编译任务的构建状态：
//! - 源码文件及其哈希
//! - 编译产物（目标文件、可执行文件、库）
//! - 编译命令历史
//! - 诊断信息

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::task::{Artifact, BuildArtifactKind};

/// 编译命令记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileCommand {
    /// 命令字符串
    pub command: String,
    /// 源文件路径
    pub source: PathBuf,
    /// 输出文件路径
    pub output: PathBuf,
    /// 是否成功
    pub success: bool,
    /// 执行时间戳
    pub timestamp: u64,
}

/// 诊断信息（编译错误/警告）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    /// 严重程度
    pub severity: DiagnosticSeverity,
    /// 文件路径
    pub file: Option<PathBuf>,
    /// 行号
    pub line: Option<u32>,
    /// 列号
    pub column: Option<u32>,
    /// 消息
    pub message: String,
}

/// 诊断严重程度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Note,
}

/// 构建状态
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildState {
    /// 源码文件 -> 内容哈希
    pub source_files: HashMap<PathBuf, String>,
    /// 目标文件列表
    pub object_files: Vec<PathBuf>,
    /// 可执行文件列表
    pub executables: Vec<PathBuf>,
    /// 静态库列表
    pub static_libraries: Vec<PathBuf>,
    /// 动态库列表
    pub dynamic_libraries: Vec<PathBuf>,
    /// 编译命令历史
    pub compile_commands: Vec<CompileCommand>,
    /// 诊断信息
    pub diagnostics: Vec<Diagnostic>,
    /// 构建目录
    pub build_dir: Option<PathBuf>,
}

impl BuildState {
    /// 创建新的构建状态
    pub fn new(build_dir: Option<PathBuf>) -> Self {
        Self {
            build_dir,
            ..Default::default()
        }
    }

    /// 从产物列表恢复构建状态
    pub fn from_artifacts(artifacts: &[Artifact]) -> Self {
        let mut state = Self::default();

        for artifact in artifacts {
            if let super::task::ArtifactKind::BuildArtifact(kind) = &artifact.kind
                && let Some(path) = &artifact.path
            {
                let path = PathBuf::from(path);
                match kind {
                    BuildArtifactKind::SourceFile => {
                        if let Some(content) = &artifact.content {
                            state.source_files.insert(path, compute_hash(content));
                        }
                    }
                    BuildArtifactKind::ObjectFile => {
                        state.object_files.push(path);
                    }
                    BuildArtifactKind::Executable => {
                        state.executables.push(path);
                    }
                    BuildArtifactKind::StaticLibrary => {
                        state.static_libraries.push(path);
                    }
                    BuildArtifactKind::DynamicLibrary => {
                        state.dynamic_libraries.push(path);
                    }
                    _ => {}
                }
            }
        }

        state
    }

    /// 检查是否需要重新编译
    pub fn needs_recompile(&self, source: &Path, new_content: &str) -> bool {
        let new_hash = compute_hash(new_content);

        match self.source_files.get(source) {
            None => true,                            // 新文件
            Some(old_hash) => old_hash != &new_hash, // 内容变更
        }
    }

    /// 记录编译命令
    pub fn record_compilation(&mut self, cmd: CompileCommand) {
        // 如果成功，更新源文件哈希
        if cmd.success {
            if let Ok(content) = std::fs::read_to_string(&cmd.source) {
                self.source_files
                    .insert(cmd.source.clone(), compute_hash(&content));
            }

            // 记录产物
            if let Some(ext) = cmd.output.extension() {
                let path = cmd.output.clone();
                match ext.to_str() {
                    Some("o") | Some("obj") => {
                        if !self.object_files.contains(&path) {
                            self.object_files.push(path);
                        }
                    }
                    Some("exe") | Some("") if path.extension().is_none() => {
                        if !self.executables.contains(&path) {
                            self.executables.push(path);
                        }
                    }
                    Some("a") | Some("lib") => {
                        if !self.static_libraries.contains(&path) {
                            self.static_libraries.push(path);
                        }
                    }
                    Some("so") | Some("dll") | Some("dylib") => {
                        if !self.dynamic_libraries.contains(&path) {
                            self.dynamic_libraries.push(path);
                        }
                    }
                    _ => {}
                }
            }
        }

        self.compile_commands.push(cmd);
    }

    /// 添加诊断信息
    pub fn add_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// 查找可执行文件
    pub fn find_executable(&self, name: &str) -> Option<&PathBuf> {
        self.executables.iter().find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s == name)
                .unwrap_or(false)
        })
    }

    /// 查找目标文件
    pub fn find_object_file(&self, name: &str) -> Option<&PathBuf> {
        self.object_files.iter().find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s == name)
                .unwrap_or(false)
        })
    }

    /// 生成 compile_commands.json（用于 IDE 集成）
    pub fn generate_compile_commands_json(&self) -> String {
        let entries: Vec<_> = self
            .compile_commands
            .iter()
            .map(|cmd| {
                serde_json::json!({
                    "directory": self.build_dir.as_ref()
                        .map(|p| p.to_str().unwrap_or(""))
                        .unwrap_or(""),
                    "command": cmd.command,
                    "file": cmd.source.to_str().unwrap_or(""),
                    "output": cmd.output.to_str().unwrap_or(""),
                })
            })
            .collect();

        serde_json::to_string_pretty(&entries).unwrap_or_default()
    }

    /// 获取所有构建产物路径
    pub fn all_artifacts(&self) -> Vec<&PathBuf> {
        self.object_files
            .iter()
            .chain(&self.executables)
            .chain(&self.static_libraries)
            .chain(&self.dynamic_libraries)
            .collect()
    }

    /// 清除诊断信息
    pub fn clear_diagnostics(&mut self) {
        self.diagnostics.clear();
    }

    /// 是否有错误诊断
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error)
    }

    /// 记录源文件及其内容哈希
    pub fn record_source_file(&mut self, path: &Path, content: &str) {
        let hash = compute_hash(content);
        self.source_files.insert(path.to_path_buf(), hash);
    }

    /// 添加目标文件
    pub fn add_object_file(&mut self, path: PathBuf) {
        if !self.object_files.contains(&path) {
            self.object_files.push(path);
        }
    }

    /// 添加可执行文件
    pub fn add_executable(&mut self, path: PathBuf) {
        if !self.executables.contains(&path) {
            self.executables.push(path);
        }
    }

    /// 添加静态库
    pub fn add_static_library(&mut self, path: PathBuf) {
        if !self.static_libraries.contains(&path) {
            self.static_libraries.push(path);
        }
    }

    /// 添加动态库
    pub fn add_dynamic_library(&mut self, path: PathBuf) {
        if !self.dynamic_libraries.contains(&path) {
            self.dynamic_libraries.push(path);
        }
    }

    /// 记录编译命令（简化版）
    pub fn record_compile_command(&mut self, command: &str, source: &Path, output: &Path) {
        let cmd = CompileCommand {
            command: command.to_string(),
            source: source.to_path_buf(),
            output: output.to_path_buf(),
            success: true,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.record_compilation(cmd);
    }

    /// 设置构建目录
    pub fn set_build_dir(&mut self, path: PathBuf) {
        self.build_dir = Some(path);
    }
}

/// 计算内容哈希
fn compute_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_state_default() {
        let state = BuildState::default();
        assert!(state.source_files.is_empty());
        assert!(state.object_files.is_empty());
        assert!(state.executables.is_empty());
        assert!(!state.has_errors());
    }

    #[test]
    fn test_needs_recompile_new_file() {
        let state = BuildState::default();
        assert!(state.needs_recompile(Path::new("test.cpp"), "content"));
    }

    #[test]
    fn test_needs_recompile_same_content() {
        let mut state = BuildState::default();
        let content = "int main() {}";
        let hash = compute_hash(content);
        state.source_files.insert(PathBuf::from("test.cpp"), hash);

        assert!(!state.needs_recompile(Path::new("test.cpp"), content));
    }

    #[test]
    fn test_find_executable() {
        let mut state = BuildState::default();
        state.executables.push(PathBuf::from("/build/hello"));
        state.executables.push(PathBuf::from("/build/world.exe"));

        assert_eq!(
            state.find_executable("hello"),
            Some(&PathBuf::from("/build/hello"))
        );
        assert_eq!(
            state.find_executable("world"),
            Some(&PathBuf::from("/build/world.exe"))
        );
        assert_eq!(state.find_executable("missing"), None);
    }
}

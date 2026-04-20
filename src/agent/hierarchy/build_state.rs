//! 构建状态管理（编译任务专用）
//!
//! 用于追踪 C++/Rust 等编译任务的构建状态：
//! - 源码文件及其哈希
//! - 编译产物（目标文件、可执行文件、库）
//! - 编译命令历史
//! - 诊断信息
//! - 增量编译支持（依赖追踪、产物缓存）

use std::collections::{HashMap, HashSet};
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
    /// 源文件哈希（用于增量编译）
    pub source_hash: Option<String>,
    /// 依赖文件列表（头文件等）
    pub dependencies: Vec<PathBuf>,
}

/// 产物缓存条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactCacheEntry {
    /// 产物路径
    pub path: PathBuf,
    /// 产物哈希
    pub hash: String,
    /// 源文件路径
    pub source: PathBuf,
    /// 源文件哈希
    pub source_hash: String,
    /// 依赖文件哈希映射
    pub dep_hashes: HashMap<PathBuf, String>,
    /// 编译命令
    pub compile_command: String,
    /// 创建时间戳
    pub created_at: u64,
    /// 最后访问时间戳
    pub last_accessed: u64,
}

/// 源文件依赖关系
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDependencies {
    /// 源文件路径
    pub source: PathBuf,
    /// 直接依赖的头文件/模块
    pub direct_deps: Vec<PathBuf>,
    /// 传递依赖（依赖的依赖）
    pub transitive_deps: Vec<PathBuf>,
    /// 最后更新时间
    pub last_updated: u64,
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
    /// 产物缓存（路径 -> 缓存条目）
    pub artifact_cache: HashMap<PathBuf, ArtifactCacheEntry>,
    /// 源文件依赖关系
    pub source_dependencies: HashMap<PathBuf, SourceDependencies>,
    /// 缓存大小限制（字节）
    pub cache_size_limit: Option<u64>,
    /// 当前缓存大小（字节）
    pub current_cache_size: u64,
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
        let source_hash = std::fs::read_to_string(source)
            .ok()
            .map(|content| compute_hash(&content));

        let cmd = CompileCommand {
            command: command.to_string(),
            source: source.to_path_buf(),
            output: output.to_path_buf(),
            success: true,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            source_hash,
            dependencies: Vec::new(),
        };
        self.record_compilation(cmd);
    }

    /// 设置构建目录
    pub fn set_build_dir(&mut self, path: PathBuf) {
        self.build_dir = Some(path);
    }

    // ==================== 增量编译支持 ====================

    /// 检查源文件及其依赖是否需要重新编译（增强版）
    ///
    /// 考虑因素：
    /// 1. 源文件内容是否变化
    /// 2. 依赖的头文件是否变化
    /// 3. 产物缓存是否有效
    pub fn needs_recompile_with_deps(&self, source: &Path) -> bool {
        // 1. 检查源文件本身
        let current_content = match std::fs::read_to_string(source) {
            Ok(content) => content,
            Err(_) => return true, // 无法读取，需要重新编译
        };
        let current_hash = compute_hash(&current_content);

        // 如果源文件不在记录中，需要编译
        let old_hash = match self.source_files.get(source) {
            Some(h) => h,
            None => return true,
        };

        if old_hash != &current_hash {
            return true; // 源文件内容变化
        }

        // 2. 检查依赖文件
        if let Some(deps) = self.source_dependencies.get(source) {
            for dep_path in &deps.direct_deps {
                if let Ok(dep_content) = std::fs::read_to_string(dep_path) {
                    let dep_hash = compute_hash(&dep_content);
                    // 检查缓存中记录的依赖哈希
                    if let Some(entry) = self.artifact_cache.values().find(|e| e.source == source)
                        && let Some(cached_dep_hash) = entry.dep_hashes.get(dep_path)
                        && cached_dep_hash != &dep_hash
                    {
                        return true; // 依赖文件变化
                    }
                }
            }
        }

        // 3. 检查产物是否存在
        if let Some(output) = self.find_expected_output(source)
            && !output.exists()
        {
            return true; // 产物不存在
        }

        false // 不需要重新编译
    }

    /// 查找源文件对应的预期输出产物
    fn find_expected_output(&self, source: &Path) -> Option<PathBuf> {
        let source_stem = source.file_stem()?.to_str()?;

        // 在产物缓存中查找
        for entry in self.artifact_cache.values() {
            if entry.source == source {
                return Some(entry.path.clone());
            }
        }

        // 在编译命令历史中查找
        for cmd in &self.compile_commands {
            if cmd.source.file_stem()?.to_str()? == source_stem {
                return Some(cmd.output.clone());
            }
        }

        None
    }

    /// 记录源文件依赖关系
    pub fn record_source_dependencies(&mut self, source: &Path, direct_deps: Vec<PathBuf>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // 计算传递依赖
        let mut transitive_deps = HashSet::new();
        for dep in &direct_deps {
            if let Some(existing) = self.source_dependencies.get(dep) {
                transitive_deps.extend(existing.direct_deps.clone());
                transitive_deps.extend(existing.transitive_deps.clone());
            }
        }

        let deps = SourceDependencies {
            source: source.to_path_buf(),
            direct_deps,
            transitive_deps: transitive_deps.into_iter().collect(),
            last_updated: now,
        };

        self.source_dependencies.insert(source.to_path_buf(), deps);
    }

    /// 缓存构建产物
    pub fn cache_artifact(
        &mut self,
        artifact_path: &Path,
        source: &Path,
        compile_command: &str,
    ) -> Result<(), String> {
        // 计算产物哈希
        let artifact_hash = match compute_file_hash(artifact_path) {
            Some(h) => h,
            None => return Err("Failed to compute artifact hash".to_string()),
        };

        // 计算源文件哈希
        let source_hash = match std::fs::read_to_string(source) {
            Ok(content) => compute_hash(&content),
            Err(_) => return Err("Failed to read source file".to_string()),
        };

        // 计算依赖哈希
        let mut dep_hashes = HashMap::new();
        if let Some(deps) = self.source_dependencies.get(source) {
            for dep in &deps.direct_deps {
                if let Ok(content) = std::fs::read_to_string(dep) {
                    dep_hashes.insert(dep.clone(), compute_hash(&content));
                }
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let entry = ArtifactCacheEntry {
            path: artifact_path.to_path_buf(),
            hash: artifact_hash,
            source: source.to_path_buf(),
            source_hash,
            dep_hashes,
            compile_command: compile_command.to_string(),
            created_at: now,
            last_accessed: now,
        };

        // 更新缓存大小
        if let Ok(metadata) = std::fs::metadata(artifact_path) {
            self.current_cache_size += metadata.len();
        }

        self.artifact_cache
            .insert(artifact_path.to_path_buf(), entry);

        // 检查是否需要清理缓存
        self.maybe_evict_cache();

        Ok(())
    }

    /// 检查缓存条目是否有效
    pub fn is_cache_valid(&self, artifact_path: &Path) -> bool {
        let entry = match self.artifact_cache.get(artifact_path) {
            Some(e) => e,
            None => return false,
        };

        // 1. 检查产物是否存在且哈希匹配
        let current_hash = match compute_file_hash(artifact_path) {
            Some(h) => h,
            None => return false,
        };
        if current_hash != entry.hash {
            return false;
        }

        // 2. 检查源文件哈希
        let current_source_hash = match std::fs::read_to_string(&entry.source) {
            Ok(content) => compute_hash(&content),
            Err(_) => return false,
        };
        if current_source_hash != entry.source_hash {
            return false;
        }

        // 3. 检查依赖哈希
        for (dep_path, cached_hash) in &entry.dep_hashes {
            let current_dep_hash = match std::fs::read_to_string(dep_path) {
                Ok(content) => compute_hash(&content),
                Err(_) => return false,
            };
            if current_dep_hash != *cached_hash {
                return false;
            }
        }

        true
    }

    /// 获取缓存条目（会更新访问时间）
    pub fn get_cached_artifact(&mut self, artifact_path: &Path) -> Option<&ArtifactCacheEntry> {
        if let Some(entry) = self.artifact_cache.get_mut(artifact_path) {
            entry.last_accessed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            Some(entry)
        } else {
            None
        }
    }

    /// 清理过期缓存
    fn maybe_evict_cache(&mut self) {
        let limit = match self.cache_size_limit {
            Some(l) => l,
            None => return, // 无限制
        };

        if self.current_cache_size <= limit {
            return; // 未超过限制
        }

        // 按最后访问时间排序，移除最久未访问的
        let mut paths: Vec<_> = self.artifact_cache.keys().cloned().collect();
        paths.sort_by(|a, b| {
            let a_time = self
                .artifact_cache
                .get(a)
                .map(|e| e.last_accessed)
                .unwrap_or(0);
            let b_time = self
                .artifact_cache
                .get(b)
                .map(|e| e.last_accessed)
                .unwrap_or(0);
            a_time.cmp(&b_time)
        });

        let mut size_to_free = self.current_cache_size - limit;
        for path in paths {
            if size_to_free == 0 {
                break;
            }

            // 从缓存中移除
            if let Some(removed) = self.artifact_cache.remove(&path)
                && let Ok(metadata) = std::fs::metadata(&removed.path)
            {
                let size = metadata.len();
                if size <= size_to_free {
                    size_to_free -= size;
                    self.current_cache_size -= size;
                }
            }
        }
    }

    /// 设置缓存大小限制
    pub fn set_cache_limit(&mut self, limit_bytes: u64) {
        self.cache_size_limit = Some(limit_bytes);
        self.maybe_evict_cache();
    }

    /// 获取缓存统计信息
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            entry_count: self.artifact_cache.len(),
            total_size: self.current_cache_size,
            size_limit: self.cache_size_limit,
            hit_rate: None, // 需要额外统计
        }
    }

    /// 清除所有缓存
    pub fn clear_cache(&mut self) {
        self.artifact_cache.clear();
        self.current_cache_size = 0;
    }

    // ==================== 持久化支持 ====================

    /// 保存构建状态到磁盘
    ///
    /// 默认保存到 `.crabmate/build_state.json`
    pub fn save_to_disk(&self, workspace_dir: &Path) -> Result<(), BuildStateError> {
        let crabmate_dir = workspace_dir.join(".crabmate");
        std::fs::create_dir_all(&crabmate_dir).map_err(|e| {
            BuildStateError::IoError(format!("Failed to create .crabmate directory: {}", e))
        })?;

        let state_path = crabmate_dir.join("build_state.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            BuildStateError::SerializeError(format!("Failed to serialize build state: {}", e))
        })?;

        std::fs::write(&state_path, json)
            .map_err(|e| BuildStateError::IoError(format!("Failed to write build state: {}", e)))?;

        log::info!(
            target: "crabmate",
            "BuildState saved to {}",
            state_path.display()
        );

        Ok(())
    }

    /// 从磁盘加载构建状态
    ///
    /// 从 `.crabmate/build_state.json` 加载
    pub fn load_from_disk(workspace_dir: &Path) -> Result<Self, BuildStateError> {
        let state_path = workspace_dir.join(".crabmate").join("build_state.json");

        if !state_path.exists() {
            return Err(BuildStateError::NotFound(format!(
                "Build state file not found: {}",
                state_path.display()
            )));
        }

        let json = std::fs::read_to_string(&state_path)
            .map_err(|e| BuildStateError::IoError(format!("Failed to read build state: {}", e)))?;

        let state: BuildState = serde_json::from_str(&json).map_err(|e| {
            BuildStateError::DeserializeError(format!("Failed to deserialize build state: {}", e))
        })?;

        log::info!(
            target: "crabmate",
            "BuildState loaded from {} ({} source files, {} artifacts cached)",
            state_path.display(),
            state.source_files.len(),
            state.artifact_cache.len()
        );

        Ok(state)
    }

    /// 尝试从磁盘加载，如果不存在则创建新的
    pub fn load_or_create(workspace_dir: &Path) -> Self {
        match Self::load_from_disk(workspace_dir) {
            Ok(state) => state,
            Err(e) => {
                log::info!(
                    target: "crabmate",
                    "Failed to load build state: {}, creating new one",
                    e
                );
                Self::default()
            }
        }
    }

    /// 删除持久化的构建状态
    pub fn remove_from_disk(workspace_dir: &Path) -> Result<(), BuildStateError> {
        let state_path = workspace_dir.join(".crabmate").join("build_state.json");
        if state_path.exists() {
            std::fs::remove_file(&state_path).map_err(|e| {
                BuildStateError::IoError(format!("Failed to remove build state: {}", e))
            })?;
            log::info!(
                target: "crabmate",
                "BuildState removed from {}",
                state_path.display()
            );
        }
        Ok(())
    }

    /// 检查是否存在持久化的构建状态
    pub fn exists_on_disk(workspace_dir: &Path) -> bool {
        workspace_dir
            .join(".crabmate")
            .join("build_state.json")
            .exists()
    }
}

/// 构建状态错误
#[derive(Debug, Clone)]
pub enum BuildStateError {
    /// IO 错误
    IoError(String),
    /// 序列化错误
    SerializeError(String),
    /// 反序列化错误
    DeserializeError(String),
    /// 文件不存在
    NotFound(String),
}

impl std::fmt::Display for BuildStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildStateError::IoError(s) => write!(f, "IO error: {}", s),
            BuildStateError::SerializeError(s) => write!(f, "Serialize error: {}", s),
            BuildStateError::DeserializeError(s) => write!(f, "Deserialize error: {}", s),
            BuildStateError::NotFound(s) => write!(f, "Not found: {}", s),
        }
    }
}

impl std::error::Error for BuildStateError {}

/// 缓存统计信息
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// 缓存条目数
    pub entry_count: usize,
    /// 当前缓存大小（字节）
    pub total_size: u64,
    /// 大小限制
    pub size_limit: Option<u64>,
    /// 命中率（可选）
    pub hit_rate: Option<f64>,
}

/// 计算内容哈希
fn compute_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// 计算文件哈希
fn compute_file_hash(path: &Path) -> Option<String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::io::Read;

    let mut file = std::fs::File::open(path).ok()?;
    let mut content = Vec::new();
    file.read_to_end(&mut content).ok()?;

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    Some(format!("{:x}", hasher.finish()))
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

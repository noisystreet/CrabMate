//! 分层执行会话状态持久化
//!
//! 提供会话级别的状态管理，包括：
//! - 已完成任务的追踪
//! - 产物状态缓存
//! - 构建状态持久化
//! - 避免重复执行

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::build_state::BuildState;
use super::task::{TaskResult, TaskStatus};

/// 会话状态文件路径
pub fn session_state_path(workspace: &Path) -> PathBuf {
    workspace
        .join(".crabmate")
        .join("hierarchical_session.json")
}

/// 已完成的任务记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedTask {
    /// 任务ID
    pub task_id: String,
    /// 任务描述（用于匹配相似任务）
    pub task_description: String,
    /// 任务状态
    pub status: TaskStatus,
    /// 完成时间
    pub completed_at: DateTime<Utc>,
    /// 产物路径列表
    pub artifacts: Vec<PathBuf>,
    /// 执行耗时（毫秒）
    pub duration_ms: u64,
}

/// 产物状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactStatus {
    /// 产物路径
    pub path: PathBuf,
    /// 文件大小
    pub size: u64,
    /// 最后修改时间
    pub modified_at: DateTime<Utc>,
    /// 内容哈希（用于验证完整性）
    pub content_hash: Option<String>,
    /// 产物类型
    pub kind: ArtifactKind,
}

/// 产物类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArtifactKind {
    Executable,
    Library,
    ObjectFile,
    SourceFile,
    ConfigFile,
    DataFile,
    Other(String),
}

/// 分层执行会话状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchicalSessionState {
    /// 会话ID
    pub session_id: String,
    /// 工作目录
    pub workspace: PathBuf,
    /// 已完成的任务
    pub completed_tasks: Vec<CompletedTask>,
    /// 产物状态（路径 -> 状态）
    pub artifact_status: HashMap<PathBuf, ArtifactStatus>,
    /// 构建状态
    pub build_state: BuildState,
    /// 最后更新时间
    pub last_updated: DateTime<Utc>,
    /// 会话版本（用于迁移）
    pub version: u32,
}

impl Default for HierarchicalSessionState {
    fn default() -> Self {
        Self {
            session_id: format!("session_{}", Utc::now().timestamp_millis()),
            workspace: PathBuf::from("."),
            completed_tasks: Vec::new(),
            artifact_status: HashMap::new(),
            build_state: BuildState::default(),
            last_updated: Utc::now(),
            version: 1,
        }
    }
}

impl HierarchicalSessionState {
    /// 创建新的会话状态
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            ..Default::default()
        }
    }

    /// 从磁盘加载会话状态
    pub fn load(workspace: &Path) -> Option<Self> {
        let path = session_state_path(workspace);
        let data = std::fs::read_to_string(&path).ok()?;
        let state: HierarchicalSessionState = serde_json::from_str(&data).ok()?;

        // 验证产物是否仍然存在
        let mut state = state;
        state.verify_artifacts();

        Some(state)
    }

    /// 保存会话状态到磁盘
    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = session_state_path(&self.workspace);

        // 确保目录存在
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, data)?;

        Ok(())
    }

    /// 记录完成的任务
    pub fn record_completed_task(&mut self, result: &TaskResult) {
        let task = CompletedTask {
            task_id: result.task_id.clone(),
            task_description: String::new(), // 由调用者填充
            status: result.status.clone(),
            completed_at: Utc::now(),
            artifacts: result
                .artifacts
                .iter()
                .filter_map(|a| a.path.as_ref().map(std::path::PathBuf::from))
                .collect(),
            duration_ms: result.duration_ms,
        };

        // 如果任务已存在，更新它
        if let Some(existing) = self
            .completed_tasks
            .iter_mut()
            .find(|t| t.task_id == task.task_id)
        {
            *existing = task;
        } else {
            self.completed_tasks.push(task);
        }

        self.last_updated = Utc::now();
        self.update_artifact_status(result);
    }

    /// 更新产物状态
    fn update_artifact_status(&mut self, result: &TaskResult) {
        for artifact in &result.artifacts {
            if let Some(ref path_str) = artifact.path {
                let path = std::path::PathBuf::from(path_str);
                if let Ok(metadata) = std::fs::metadata(&path)
                    && let Ok(modified) = metadata.modified()
                {
                    let modified_at = DateTime::from(modified);
                    let status = ArtifactStatus {
                        path: path.clone(),
                        size: metadata.len(),
                        modified_at,
                        content_hash: None, // 可以添加哈希计算
                        kind: Self::infer_artifact_kind(&path),
                    };
                    self.artifact_status.insert(path, status);
                }
            }
        }
    }

    /// 推断产物类型
    fn infer_artifact_kind(path: &Path) -> ArtifactKind {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let file_name = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "exe" | "" => {
                if file_name.starts_with("lib") {
                    ArtifactKind::Library
                } else {
                    ArtifactKind::Executable
                }
            }
            "so" | "dylib" | "dll" | "a" | "lib" => ArtifactKind::Library,
            "o" | "obj" => ArtifactKind::ObjectFile,
            "c" | "cpp" | "cc" | "h" | "hpp" | "rs" | "py" | "js" | "ts" => {
                ArtifactKind::SourceFile
            }
            "toml" | "yaml" | "yml" | "json" | "xml" | "ini" | "conf" => ArtifactKind::ConfigFile,
            "dat" | "txt" | "csv" => ArtifactKind::DataFile,
            _ => ArtifactKind::Other(ext),
        }
    }

    /// 验证产物是否仍然存在
    fn verify_artifacts(&mut self) {
        let to_remove: Vec<PathBuf> = self
            .artifact_status
            .iter()
            .filter(|(_, status)| !status.path.exists())
            .map(|(path, _)| path.clone())
            .collect();

        for path in to_remove {
            self.artifact_status.remove(&path);
        }

        // 清理不存在的产物引用
        for task in &mut self.completed_tasks {
            task.artifacts.retain(|p| p.exists());
        }
    }

    /// 查找已完成的相似任务
    pub fn find_completed_task(&self, description: &str) -> Option<&CompletedTask> {
        let desc_lower = description.to_lowercase();

        self.completed_tasks.iter().find(|task| {
            let task_desc_lower = task.task_description.to_lowercase();

            // 简单匹配：检查描述是否包含关键词
            let keywords = extract_keywords(&desc_lower);
            let task_keywords = extract_keywords(&task_desc_lower);

            // 如果关键词重叠度高，认为是相似任务
            let overlap = keywords
                .iter()
                .filter(|k| task_keywords.contains(k))
                .count();

            overlap > 0 && overlap * 2 >= keywords.len()
        })
    }

    /// 检查产物是否存在且有效
    pub fn verify_artifacts_exist(&self, paths: &[PathBuf]) -> bool {
        paths.iter().all(|p| {
            if let Some(status) = self.artifact_status.get(p) {
                p.exists() && Self::verify_artifact_integrity(p, status)
            } else {
                p.exists()
            }
        })
    }

    /// 验证产物完整性
    fn verify_artifact_integrity(path: &Path, status: &ArtifactStatus) -> bool {
        if let Ok(metadata) = std::fs::metadata(path) {
            // 检查大小是否变化
            if metadata.len() != status.size {
                return false;
            }

            // 检查修改时间
            if let Ok(modified) = metadata.modified() {
                let modified_at: DateTime<Utc> = DateTime::from(modified);
                return modified_at == status.modified_at;
            }
        }
        true
    }

    /// 获取已完成的编译类任务
    pub fn get_completed_compilations(&self) -> Vec<&CompletedTask> {
        self.completed_tasks
            .iter()
            .filter(|t| {
                let desc = t.task_description.to_lowercase();
                desc.contains("编译") || desc.contains("build") || desc.contains("make")
            })
            .collect()
    }

    /// 检查可执行文件是否已存在
    pub fn is_executable_built(&self, name: &str) -> Option<&Path> {
        let exe_name = name.to_lowercase();

        for (path, status) in &self.artifact_status {
            if let ArtifactKind::Executable = status.kind
                && let Some(file_name) = path.file_stem()
                && file_name.to_string_lossy().to_lowercase() == exe_name
            {
                return Some(path);
            }
        }
        None
    }

    /// 清空会话状态
    pub fn clear(&mut self) {
        self.completed_tasks.clear();
        self.artifact_status.clear();
        self.build_state = BuildState::default();
        self.last_updated = Utc::now();
    }
}

/// 提取关键词（简单实现）
fn extract_keywords(text: &str) -> Vec<String> {
    // 定义停用词列表
    let stop_words: std::collections::HashSet<&str> = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "and", "or", "but", "in", "on",
        "at", "to", "for", "of", "with", "的", "了", "在", "是", "和", "有", "我", "他", "她",
        "它",
    ]
    .iter()
    .cloned()
    .collect();

    text.split_whitespace()
        .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|s| {
            let s_lower = s.to_lowercase();
            !s.is_empty() && s.len() > 1 && !stop_words.contains(s_lower.as_str())
        })
        .map(|s| s.to_lowercase())
        .collect()
}

/// 线程安全的会话状态管理器
pub struct SessionStateManager {
    state: Arc<Mutex<HierarchicalSessionState>>,
    auto_save: bool,
}

impl SessionStateManager {
    /// 创建新的会话状态管理器
    pub fn new(workspace: PathBuf, auto_save: bool) -> Self {
        // 尝试加载已有状态，否则创建新的
        let state = HierarchicalSessionState::load(&workspace)
            .unwrap_or_else(|| HierarchicalSessionState::new(workspace));

        Self {
            state: Arc::new(Mutex::new(state)),
            auto_save,
        }
    }

    /// 获取状态（只读）
    pub fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&HierarchicalSessionState) -> R,
    {
        let state = self.state.lock().unwrap();
        f(&state)
    }

    /// 修改状态
    pub fn modify_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HierarchicalSessionState) -> R,
    {
        let mut state = self.state.lock().unwrap();
        let result = f(&mut state);

        if self.auto_save {
            let _ = state.save();
        }

        result
    }

    /// 记录完成的任务
    pub fn record_task(&self, result: &TaskResult) {
        self.modify_state(|state| {
            state.record_completed_task(result);
        });
    }

    /// 检查任务是否需要执行
    pub fn should_execute_task(&self, description: &str) -> bool {
        self.with_state(|state| {
            if let Some(completed) = state.find_completed_task(description) {
                // 检查产物是否仍然存在
                if state.verify_artifacts_exist(&completed.artifacts) {
                    log::info!(
                        target: "crabmate",
                        "[SESSION] Task '{}' already completed, skipping",
                        description
                    );
                    return false;
                }
            }
            true
        })
    }

    /// 检查可执行文件是否已构建
    pub fn is_executable_built(&self, name: &str) -> Option<PathBuf> {
        self.with_state(|state| state.is_executable_built(name).map(|p| p.to_path_buf()))
    }

    /// 保存状态
    pub fn save(&self) -> Result<(), std::io::Error> {
        self.with_state(|state| state.save())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_session_state_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path();

        // 创建并保存状态
        let mut state = HierarchicalSessionState::new(workspace.to_path_buf());
        state.session_id = "test_session".to_string();
        state.save().unwrap();

        // 加载状态
        let loaded = HierarchicalSessionState::load(workspace).unwrap();
        assert_eq!(loaded.session_id, "test_session");
    }

    #[test]
    fn test_find_completed_task() {
        let mut state = HierarchicalSessionState::default();

        state.completed_tasks.push(CompletedTask {
            task_id: "task1".to_string(),
            task_description: "编译 hpcg 源码".to_string(),
            status: TaskStatus::Completed,
            completed_at: Utc::now(),
            artifacts: vec![],
            duration_ms: 1000,
        });

        // 应该能找到相似任务（关键词匹配）
        let found = state.find_completed_task("编译 hpcg 程序");
        assert!(found.is_some());

        // 不相关的任务不应该匹配
        let not_found = state.find_completed_task("运行测试");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_infer_artifact_kind() {
        use std::path::Path;

        assert!(matches!(
            HierarchicalSessionState::infer_artifact_kind(Path::new("test.exe")),
            ArtifactKind::Executable
        ));

        assert!(matches!(
            HierarchicalSessionState::infer_artifact_kind(Path::new("libtest.so")),
            ArtifactKind::Library
        ));

        assert!(matches!(
            HierarchicalSessionState::infer_artifact_kind(Path::new("test.cpp")),
            ArtifactKind::SourceFile
        ));
    }
}

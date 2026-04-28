//! 会话可执行体检索与编译类任务快捷路径。

use super::super::task::ExecutionStrategy;
use super::types::ManagerAgent;

impl ManagerAgent {
    pub fn execution_strategy(&self) -> ExecutionStrategy {
        self.config.execution_strategy
    }

    /// 检查是否是编译类任务
    pub(super) fn is_compile_task(&self, task: &str) -> bool {
        let task_lower = task.to_lowercase();
        task_lower.contains("编译")
            || task_lower.contains("build")
            || task_lower.contains("make")
            || task_lower.contains("cmake")
    }

    /// 从任务描述中提取可执行文件名
    pub(super) fn extract_executable_name(&self, task: &str) -> Option<String> {
        let task_lower = task.to_lowercase();

        let patterns = [
            r"编译\s+(\w+)",
            r"build\s+(\w+)",
            r"make\s+(\w+)",
            r"编译\s+(\w+)\s+源码",
            r"编译\s+(\w+)\s+源代码",
        ];

        for pattern in &patterns {
            if let Ok(re) = regex::Regex::new(pattern)
                && let Some(cap) = re.captures(&task_lower)
                && let Some(name) = cap.get(1)
            {
                return Some(name.as_str().to_lowercase());
            }
        }

        if let Ok(re) = regex::Regex::new(r"(\w+)[-_].*\.(tar\.gz|tgz|zip)")
            && let Some(cap) = re.captures(&task_lower)
            && let Some(name) = cap.get(1)
        {
            return Some(name.as_str().to_lowercase());
        }

        None
    }

    /// 检查可执行文件是否已存在
    pub(super) fn check_existing_executable(&self, task: &str) -> Option<std::path::PathBuf> {
        if let Some(name) = self.extract_executable_name(task) {
            if let Some(path) = self.is_executable_built(&name) {
                return Some(path);
            }

            let common_paths = [
                format!("{}/bin/{}", name, name),
                format!("{}/bin/x{}", name, name),
                format!("{}/{}", name, name),
                format!("bin/{}", name),
                format!("build/{}", name),
                name.clone(),
            ];

            for path_str in &common_paths {
                let path = std::path::Path::new(path_str);
                if path.exists() && path.is_file() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(metadata) = path.metadata() {
                            let permissions = metadata.permissions();
                            if permissions.mode() & 0o111 != 0 {
                                return Some(path.to_path_buf());
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        return Some(path.to_path_buf());
                    }
                }
            }
        }

        None
    }
}

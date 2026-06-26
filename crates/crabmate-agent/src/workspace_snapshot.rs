use std::path::Path;
use std::process::Command;

pub struct WorkspaceSnapshot {
    workspace_root: std::path::PathBuf,
    tree_hash: String,
    index_file: std::path::PathBuf,
}

impl WorkspaceSnapshot {
    /// 尝试在指定工作区创建快照。如果不是 Git 仓库，则返回 None。
    pub fn take(workspace_root: &Path) -> Option<Self> {
        // 1. 检查是否是 git 仓库
        let status = Command::new("git")
            .arg("rev-parse")
            .arg("--is-inside-work-tree")
            .current_dir(workspace_root)
            .output()
            .ok()?;
        if !status.status.success() {
            return None;
        }

        // 2. 生成临时 index 文件路径
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let index_file = workspace_root.join(".git").join(format!(
            "crabmate_backup_{}_{}.idx",
            std::process::id(),
            timestamp
        ));

        // 3. 将当前所有文件（含未跟踪）加入临时 index
        let add_status = Command::new("git")
            .env("GIT_INDEX_FILE", &index_file)
            .arg("add")
            .arg("-A")
            .current_dir(workspace_root)
            .output()
            .ok()?;
        if !add_status.status.success() {
            let _ = std::fs::remove_file(&index_file);
            return None;
        }

        // 4. 写入 tree
        let tree_output = Command::new("git")
            .env("GIT_INDEX_FILE", &index_file)
            .arg("write-tree")
            .current_dir(workspace_root)
            .output()
            .ok()?;
        if !tree_output.status.success() {
            let _ = std::fs::remove_file(&index_file);
            return None;
        }

        let tree_hash = String::from_utf8_lossy(&tree_output.stdout)
            .trim()
            .to_string();

        Some(Self {
            workspace_root: workspace_root.to_path_buf(),
            tree_hash,
            index_file,
        })
    }

    /// 回滚工作区到快照状态
    pub fn restore(&self) -> Result<(), String> {
        // 1. 恢复被修改或删除的文件
        let checkout_status = Command::new("git")
            .env("GIT_INDEX_FILE", &self.index_file)
            .arg("checkout")
            .arg(&self.tree_hash)
            .arg("--")
            .arg(".")
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|e| e.to_string())?;

        if !checkout_status.status.success() {
            return Err(format!(
                "git checkout failed: {}",
                String::from_utf8_lossy(&checkout_status.stderr)
            ));
        }

        // 2. 清除快照后新增的未跟踪文件
        let clean_status = Command::new("git")
            .env("GIT_INDEX_FILE", &self.index_file)
            .arg("clean")
            .arg("-fd")
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|e| e.to_string())?;

        if !clean_status.status.success() {
            return Err(format!(
                "git clean failed: {}",
                String::from_utf8_lossy(&clean_status.stderr)
            ));
        }

        Ok(())
    }
}

impl Drop for WorkspaceSnapshot {
    fn drop(&mut self) {
        // 如果没有调用 restore，也需要清理临时的 index 文件
        let _ = std::fs::remove_file(&self.index_file);
    }
}

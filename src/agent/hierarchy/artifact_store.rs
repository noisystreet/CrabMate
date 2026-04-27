//! 全局产物存储

use std::collections::HashMap;

use super::task::{Artifact, TaskResult};

/// 全局产物存储
#[derive(Debug, Clone, Default)]
pub struct ArtifactStore {
    /// artifact_id -> Artifact
    artifacts: HashMap<String, Artifact>,
    /// goal_id -> 产生的 artifact_ids
    produced_by: HashMap<String, Vec<String>>,
}

impl ArtifactStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 存储 artifact
    pub fn put(&mut self, artifact: Artifact) {
        let id = artifact.id.clone();
        let produced_by_goal = artifact.produced_by.clone();

        self.artifacts.insert(id.clone(), artifact);

        self.produced_by
            .entry(produced_by_goal.clone())
            .or_default()
            .push(id.clone());
    }

    /// 获取 artifact
    pub fn get(&self, id: &str) -> Option<&Artifact> {
        self.artifacts.get(id)
    }

    /// 获取某个 goal 产生的所有 artifacts
    pub fn get_produced_by(&self, goal_id: &str) -> Vec<&Artifact> {
        self.produced_by
            .get(goal_id)
            .map(|ids| ids.iter().filter_map(|id| self.artifacts.get(id)).collect())
            .unwrap_or_default()
    }

    /// 获取 `depends_on` 中**每个**前置子目标在 store 中登记的**全部**产物（按依赖顺序、同 goal 内与登记顺序一致）。
    ///
    /// 旧实现只取每 goal 的**第一个** artifact，编译/运行多产物场景下后续子目标会看不到可执行体、头文件等。
    pub fn get_dependencies(&self, depends_on: &[String]) -> Vec<&Artifact> {
        let mut deps = Vec::new();
        for dep_goal_id in depends_on {
            if let Some(ids) = self.produced_by.get(dep_goal_id) {
                for id in ids {
                    if let Some(artifact) = self.artifacts.get(id.as_str()) {
                        deps.push(artifact);
                    }
                }
            }
        }
        deps
    }

    /// 从 TaskResult 中提取 artifacts 并存储
    pub fn store_result(&mut self, task_result: &TaskResult) {
        for artifact in &task_result.artifacts {
            self.put(artifact.clone());
        }
    }

    /// 获取所有 artifacts
    pub fn all(&self) -> Vec<&Artifact> {
        self.artifacts.values().collect()
    }

    /// 合并另 store 的条目到 `self`（`put` 语义，重复 `id` 时后者覆盖，与 `HashMap::insert` 一致）。
    pub fn merge_from(&mut self, other: &Self) {
        for a in other.artifacts.values() {
            self.put(a.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::hierarchy::task::ArtifactKind;

    fn sample_artifact(id: &str, name: &str, produced_by: &str) -> Artifact {
        Artifact {
            id: id.to_string(),
            name: name.to_string(),
            kind: ArtifactKind::File,
            path: Some(format!("out/{id}")),
            content: None,
            metadata: serde_json::Value::Null,
            produced_by: produced_by.to_string(),
            consumed_by: vec![],
        }
    }

    #[test]
    fn get_dependencies_returns_all_artifacts_per_prerequisite_goal() {
        let mut store = ArtifactStore::new();
        let g1a = sample_artifact("a1", "file-a", "goal_1");
        let g1b = sample_artifact("a2", "file-b", "goal_1");
        let g2a = sample_artifact("b1", "out-c", "goal_2");
        store.put(g1a);
        store.put(g1b);
        store.put(g2a);

        let deps: Vec<String> = vec!["goal_1".to_string(), "goal_2".to_string()];
        let got = store.get_dependencies(&deps);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].id, "a1");
        assert_eq!(got[1].id, "a2");
        assert_eq!(got[2].id, "b1");
    }
}

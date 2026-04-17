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

    /// 获取某个 goal 的依赖 artifacts
    pub fn get_dependencies(&self, depends_on: &[String]) -> Vec<&Artifact> {
        let mut deps = Vec::new();
        for dep_goal_id in depends_on {
            if let Some(ids) = self.produced_by.get(dep_goal_id)
                && let Some(first_id) = ids.first()
                && let Some(artifact) = self.artifacts.get(first_id)
            {
                deps.push(artifact);
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
}

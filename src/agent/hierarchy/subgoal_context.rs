//! 分层子目标：I/O 契约、依赖产物过滤、上一步步级摘要

use std::collections::HashSet;

use super::task::{
    Artifact, ArtifactKind, BuildArtifactKind, DependencyContractEntry, SubGoal, TaskResult,
    TaskStatus,
};

/// 返回 `None` 表示可接受；`Some` 为**非致命**警告
pub(crate) fn validate_depends_consumes_consistency(goal: &SubGoal) -> Option<String> {
    let in_dep: HashSet<&str> = goal.depends_on.iter().map(String::as_str).collect();
    for e in &goal.consumes_from_dependencies {
        if !in_dep.contains(e.from_goal_id.as_str()) {
            return Some(format!(
                "SubGoal {} 的 consumes 含 from_goal_id={} 但不在 depends_on 中，已忽略该条",
                goal.goal_id, e.from_goal_id
            ));
        }
    }
    None
}

/// 未写 `consumes_from_dependencies` 时，为 `depends_on` 中已完成的、且（同层时）`same_layer_auto` 为真时补全。
pub(crate) fn ensure_consumes_from_dependencies(
    goal: &SubGoal,
    prior_subgoals: &[TaskResult],
    current_level_ids: &HashSet<String>,
    same_layer_auto: bool,
) -> SubGoal {
    if !goal.consumes_from_dependencies.is_empty() {
        return goal.clone();
    }
    let id_to_done: HashSet<String> = prior_subgoals
        .iter()
        .filter(|r| matches!(r.status, TaskStatus::Completed))
        .map(|r| r.task_id.clone())
        .collect();
    let mut add: Vec<DependencyContractEntry> = Vec::new();
    for dep in &goal.depends_on {
        if !id_to_done.contains(dep) {
            continue;
        }
        if current_level_ids.contains(dep) {
            if same_layer_auto {
                add.push(DependencyContractEntry {
                    from_goal_id: dep.clone(),
                    only_kinds: None,
                });
            }
        } else {
            add.push(DependencyContractEntry {
                from_goal_id: dep.clone(),
                only_kinds: None,
            });
        }
    }
    if add.is_empty() {
        return goal.clone();
    }
    let mut g = goal.clone();
    g.consumes_from_dependencies = add;
    g
}

/// `Debug( lower )` 形态
pub(crate) fn kind_lowercase(a: &Artifact) -> String {
    format!("{:?}", a.kind).to_lowercase()
}

fn only_kinds_means_all(only: &Option<Vec<String>>) -> bool {
    only.as_ref().is_some_and(|k| {
        k.iter()
            .any(|x| matches!(x.to_lowercase().as_str(), "all" | "any" | "full"))
    })
}

/// 在「默认排冗长」下省略构建日志、纯正文命令输出
pub(crate) fn should_include_artifact_for_injection(
    a: &Artifact,
    c: &DependencyContractEntry,
) -> bool {
    if let Some(ref kinds) = c.only_kinds
        && !kinds.is_empty()
    {
        if only_kinds_means_all(&c.only_kinds) {
            return true;
        }
        let ak = kind_lowercase(a);
        for want in kinds {
            let w = want.to_lowercase();
            if ak.contains(&w) {
                return true;
            }
            if w == "executable"
                && let ArtifactKind::BuildArtifact(bk) = a.kind
                && bk == BuildArtifactKind::Executable
            {
                return true;
            }
            if w == "source"
                && let ArtifactKind::BuildArtifact(bk) = a.kind
                && bk == BuildArtifactKind::SourceFile
            {
                return true;
            }
        }
        return false;
    }
    if let ArtifactKind::BuildArtifact(bk) = a.kind
        && bk == BuildArtifactKind::BuildLog
    {
        return false;
    }
    if a.path
        .as_ref()
        .is_some_and(|p| p.to_lowercase().ends_with("buildlog"))
    {
        return false;
    }
    if matches!(&a.kind, ArtifactKind::CommandOutput) {
        return a.path.is_some();
    }
    true
}

pub(crate) fn build_step_result_summary(r: &TaskResult) -> String {
    let status = match &r.status {
        TaskStatus::Completed => "completed",
        TaskStatus::Failed { .. } => "failed",
        TaskStatus::Skipped { .. } => "skipped",
        TaskStatus::NeedsDecomposition { .. } => "needs_decomposition",
        TaskStatus::Pending | TaskStatus::InProgress => "other",
    };
    let mut path_lines: Vec<String> = r
        .artifacts
        .iter()
        .filter_map(|a| a.path.as_deref().map(String::from))
        .collect();
    path_lines.sort();
    path_lines.dedup();
    let (shown, n) = if path_lines.len() > 5 {
        (path_lines[..5].join(", "), path_lines.len())
    } else {
        (path_lines.join(", "), path_lines.len())
    };
    let path_part = if n == 0 {
        "(无)".to_string()
    } else if n > 5 {
        format!("{shown} 等共 {n} 条")
    } else {
        shown
    };
    let tools = if r.tools_invoked.is_empty() {
        "(无)".to_string()
    } else {
        r.tools_invoked
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(" → ")
    };
    format!(
        "- 子目标 **`{}`**: 状态 **{status}**；耗时约 {}ms；本步已登记相对路径: {path_part}；本步工具序（前若干）: {tools}",
        r.task_id, r.duration_ms,
    )
}

pub(crate) fn build_prior_subgoals_summary_block(results: &[TaskResult], limit: usize) -> String {
    if results.is_empty() {
        return String::new();
    }
    let take = results.len().min(limit);
    let start = results.len() - take;
    let body: String = results[start..]
        .iter()
        .map(build_step_result_summary)
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "## 最近已完成子目标步摘要\n\
         供当前步衔接；**以「来自前置子目标的已登记产物」中路径为准**。\n\
         \n\
         {body}\n"
    )
}

/// 按 `consumes_from_dependencies` 与 `depends_on` 从 `get_dependencies` 的扁平列表中筛选
pub(crate) fn filter_dependencies_for_injection<'a>(
    goal: &SubGoal,
    raw: &[&'a Artifact],
) -> Vec<&'a Artifact> {
    let mut out: Vec<&'a Artifact> = Vec::new();
    for dep in &goal.depends_on {
        let default_contract = DependencyContractEntry {
            from_goal_id: dep.clone(),
            only_kinds: None,
        };
        let c = goal
            .consumes_from_dependencies
            .iter()
            .find(|e| e.from_goal_id == *dep)
            .unwrap_or(&default_contract);
        for a in raw.iter().copied().filter(|a| a.produced_by == *dep) {
            if should_include_artifact_for_injection(a, c) {
                out.push(a);
            }
        }
    }
    out
}

/// 与顺序路径 `HierarchicalExecutor::execute_single_impl` 一致的「依赖节 + 步摘要」user 附加上下文 `extra`
pub fn build_injected_subgoal_user_extra(
    goal: &SubGoal,
    dep_artifacts: &[&Artifact],
    prior_subgoals: &[TaskResult],
) -> Option<String> {
    let sum_block = build_prior_subgoals_summary_block(prior_subgoals, 8);
    if dep_artifacts.is_empty() {
        return if sum_block.is_empty() {
            None
        } else {
            Some(sum_block)
        };
    }
    let p = subgoal_io_contract_preamble_text(goal);
    let body = format_filtered_dependency_sections(&goal.depends_on, dep_artifacts);
    let dep_block = if p.is_empty() { body } else { p + &body };
    match (sum_block.is_empty(), dep_block.is_empty()) {
        (true, true) => None,
        (false, true) => Some(sum_block),
        (true, false) => Some(dep_block),
        (false, false) => Some(format!("{sum_block}\n{dep_block}")),
    }
}

/// 规划器/解析侧：从 store 中收集 `depends_on` 的扁平列表
pub fn collect_artifacts_for_goals<'a>(
    store: &'a super::artifact_store::ArtifactStore,
    depends_on: &[String],
) -> Vec<&'a Artifact> {
    let mut v = Vec::new();
    for d in depends_on {
        v.extend(store.get_produced_by(d));
    }
    v
}

fn subgoal_io_contract_preamble_text(goal: &SubGoal) -> String {
    if goal.consumes_from_dependencies.is_empty() {
        return String::new();
    }
    let mut s = String::from("## 子目标 I/O 契约（`consumes_from_dependencies`）\n");
    for c in &goal.consumes_from_dependencies {
        let k = c
            .only_kinds
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(|v| v.join(", "))
            .unwrap_or_else(|| "默认(排除 buildlog/冗长 commandoutput 等)".to_string());
        s.push_str(&format!(
            "- 自 **`{}`** 消费，类型/筛选: {k}\n",
            c.from_goal_id
        ));
    }
    s.push_str(
        "在工具调用的 **JSON 字符串参数**中可写 `{ref:<前序子目标id>:<artifact_id>}`；亦可沿用 `{artifact:文件名}`。\n\n",
    );
    s
}

fn format_filtered_dependency_sections(depends_on: &[String], deps: &[&Artifact]) -> String {
    if deps.is_empty() {
        return String::new();
    }
    let mut sections: Vec<String> = vec![String::from(
        "## 来自前置子目标的已登记产物（**类型已按契约裁剪**）\n\
         执行当前子目标时**请优先使用**下列路径；可在工具参数中使用 `{ref:<子目标id>:<artifact_id>}` 占位符。\n",
    )];
    for dep_goal_id in depends_on {
        let group: Vec<&Artifact> = deps
            .iter()
            .copied()
            .filter(|a| a.produced_by == *dep_goal_id)
            .collect();
        if group.is_empty() {
            sections.push(format!(
                "### 子目标 `{dep_goal_id}`\n- （尚无**可见**登记产物。）"
            ));
            continue;
        }
        let mut lines = vec![format!("### 子目标 `{dep_goal_id}`")];
        for a in &group {
            let kind_str = format!("{:?}", a.kind).to_lowercase();
            let r = format!("{{ref:{}:{}}}", a.produced_by, a.id);
            if let Some(ref path) = a.path {
                lines.push(format!(
                    "- `artifact_id={}` · [{}] **{}** — 工作区相对路径: `{path}` — 占位符: `{r}`",
                    a.id, kind_str, a.name
                ));
            } else if let Some(ref content) = a.content {
                let total = content.chars().count();
                let preview: String = content.chars().take(200).collect();
                let body = if total > 200 {
                    format!("{preview}... (共 {total} 字符)")
                } else {
                    preview
                };
                lines.push(format!(
                    "- `artifact_id={}` · [{}] **{}**（无 path） — 占位符: `{r}`\n  ```\n  {body}\n  ```"
                    , a.id, kind_str, a.name
                ));
            } else {
                lines.push(format!(
                    "- `artifact_id={}` · [{}] **{}**（无 path/内容）— 占位符: `{r}`",
                    a.id, kind_str, a.name
                ));
            }
        }
        sections.push(lines.join("\n"));
    }
    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::super::task::Artifact;
    use super::*;

    fn art(id: &str, goal: &str, kind: ArtifactKind) -> Artifact {
        let mut a = Artifact::new(id, id, kind, goal);
        a.path = Some(format!("p/{id}"));
        a
    }

    #[test]
    fn buildlog_dropped_by_default() {
        let a = art(
            "l",
            "g1",
            ArtifactKind::BuildArtifact(BuildArtifactKind::BuildLog),
        );
        let c = DependencyContractEntry {
            from_goal_id: "g1".into(),
            only_kinds: None,
        };
        assert!(!should_include_artifact_for_injection(&a, &c));
    }

    #[test]
    fn all_kinds_keeps_log() {
        let a = art(
            "l",
            "g1",
            ArtifactKind::BuildArtifact(BuildArtifactKind::BuildLog),
        );
        let c = DependencyContractEntry {
            from_goal_id: "g1".into(),
            only_kinds: Some(vec!["all".into()]),
        };
        assert!(should_include_artifact_for_injection(&a, &c));
    }
}

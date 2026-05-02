//! `execution.rs` 附属纯函数与 DAG（减轻 `HierarchicalExecutor` 文件体积）。

use std::collections::{HashMap, HashSet};

use super::execution_error::ExecutionError;
use super::task::{SubGoal, TaskResult};

pub(crate) fn summarize_subgoal_evidence(result: &TaskResult) -> Option<String> {
    let text = format!(
        "{}\n{}",
        result.output.as_deref().unwrap_or(""),
        result.error.as_deref().unwrap_or("")
    );
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with("[subgoal_tool_trace]")
            || line.starts_with("Tool ")
            || line.starts_with("Final:")
            || line.starts_with("参数：")
            || line.starts_with("标准输出：")
            || line.starts_with("## ")
            || line.starts_with("---")
        {
            continue;
        }
        return Some(trim_for_detail(line, 160));
    }
    None
}

pub(crate) fn trim_for_detail(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out = String::new();
    for ch in s.chars().take(max_chars.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

/// 截断目标描述用于日志（按字符边界截断，支持中文）
pub(crate) fn truncate_goal_desc(desc: &str) -> String {
    const MAX_LEN: usize = 80;
    if desc.len() > MAX_LEN {
        let truncated = desc
            .char_indices()
            .take(MAX_LEN - 3)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &desc[..truncated])
    } else {
        desc.to_string()
    }
}

/// DAG 构建器
#[derive(Debug)]
pub(crate) struct Dag {
    nodes: HashSet<String>,
    edges: HashMap<String, HashSet<String>>,
}

impl Dag {
    pub(crate) fn build(goals: &[SubGoal]) -> Result<Self, ExecutionError> {
        let mut dag = Dag {
            nodes: HashSet::new(),
            edges: HashMap::new(),
        };

        for goal in goals {
            dag.nodes.insert(goal.goal_id.clone());
            dag.edges.entry(goal.goal_id.clone()).or_default();

            for dep in &goal.depends_on {
                if !dag.nodes.contains(dep) {
                    dag.nodes.insert(dep.clone());
                    dag.edges.entry(dep.clone()).or_default();
                }
                dag.edges.get_mut(dep).unwrap().insert(goal.goal_id.clone());
            }
        }

        Ok(dag)
    }

    /// 计算拓扑层级
    pub(crate) fn topological_levels(&self) -> Result<Vec<Vec<String>>, ExecutionError> {
        let mut levels = Vec::new();
        let mut remaining = self.nodes.clone();
        let mut in_degree: HashMap<String, usize> =
            self.nodes.iter().map(|id| (id.clone(), 0)).collect();

        // 计算入度
        for targets in self.edges.values() {
            for target in targets {
                if let Some(d) = in_degree.get_mut(target) {
                    *d += 1;
                }
            }
        }

        while !remaining.is_empty() {
            // 找到入度为 0 的节点
            let level: Vec<String> = remaining
                .iter()
                .filter(|id| in_degree.get(*id) == Some(&0))
                .cloned()
                .collect();

            if level.is_empty() {
                return Err(ExecutionError::DagError(
                    "Cycle detected in dependencies".to_string(),
                ));
            }

            for id in &level {
                remaining.remove(id);
                if let Some(targets) = self.edges.get(id) {
                    for target in targets {
                        if let Some(d) = in_degree.get_mut(target) {
                            *d -= 1;
                        }
                    }
                }
            }

            levels.push(level);
        }

        Ok(levels)
    }
}

/// Manager 分解时若填写了非空 `required_tools` 却漏关键工具，Operator 会拒绝调用。按子目标描述补全常见缺口（`required_tools` 为空时仍走全量工具，不调用本函数）。
pub(crate) fn supplement_subgoal_required_tools(description: &str, tools: &mut Vec<String>) {
    if tools.is_empty() {
        return;
    }
    let d = description.to_lowercase();
    let mut push = |name: &str| {
        if !tools.iter().any(|t| t == name) {
            tools.push(name.to_string());
        }
    };
    let mentions_compile = d.contains("编译")
        || d.contains("构建")
        || d.contains("--build")
        || d.contains("链接")
        || d.contains("make")
        || d.contains("g++")
        || d.contains("clang")
        || d.contains("ninja")
        || d.contains("meson")
        || (d.contains("cmake")
            && (d.contains("执行")
                || d.contains("配置")
                || d.contains("生成")
                || d.contains("安装")));
    let mentions_run = d.contains("运行")
        || d.contains("执行")
        || d.contains("跑")
        || d.contains("验证输出")
        || d.contains("退出码")
        || d.contains("hello");
    let inspects_build_tree = (d.contains("检查") || d.contains("确认") || d.contains("验证"))
        && (d.contains("build")
            || d.contains("可执行")
            || d.contains("产物")
            || d.contains("生成"));

    if mentions_compile || inspects_build_tree {
        push("run_command");
    }
    if mentions_run {
        push("run_command");
        push("run_executable");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_levels() {
        let goals = vec![
            SubGoal::new("a", "task a"),
            SubGoal::new("b", "task b").with_depends_on(vec!["a".to_string()]),
            SubGoal::new("c", "task c").with_depends_on(vec!["a".to_string()]),
            SubGoal::new("d", "task d").with_depends_on(vec!["b".to_string(), "c".to_string()]),
        ];

        let dag = Dag::build(&goals).unwrap();
        let levels = dag.topological_levels().unwrap();

        assert_eq!(levels.len(), 3);
        assert!(levels[0].contains(&"a".to_string()));
        assert!(levels[1].contains(&"b".to_string()) || levels[1].contains(&"c".to_string()));
    }

    #[test]
    fn test_dag_cycle_detection() {
        let goals = vec![
            SubGoal::new("a", "task a").with_depends_on(vec!["b".to_string()]),
            SubGoal::new("b", "task b").with_depends_on(vec!["a".to_string()]),
        ];

        let dag = Dag::build(&goals).unwrap();
        let result = dag.topological_levels();

        assert!(result.is_err());
    }

    #[test]
    fn supplement_adds_run_command_for_inspect_build_goal() {
        let mut t = vec!["read_dir".to_string()];
        supplement_subgoal_required_tools("检查 build 目录确认可执行文件已生成", &mut t);
        assert!(t.contains(&"run_command".to_string()));
    }

    #[test]
    fn supplement_noop_when_tools_empty() {
        let mut t: Vec<String> = vec![];
        supplement_subgoal_required_tools("cmake --build build", &mut t);
        assert!(t.is_empty());
    }

    #[test]
    fn supplement_adds_run_command_for_cmake_configure_subset() {
        let mut t = vec!["mkdir".to_string(), "read_dir".to_string()];
        supplement_subgoal_required_tools("创建 build 并执行 cmake -S . -B build 配置", &mut t);
        assert!(t.contains(&"run_command".to_string()));
    }
}

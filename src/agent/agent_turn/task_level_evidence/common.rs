//! 任务级验收：共享小工具与期望输出提示（从单文件拆出，避免 lizard 对 `r#""#` 的误解析）。

use std::collections::HashMap;

use crate::agent::hierarchy::task::TaskResult;

pub(super) fn cpp_source_path(p: Option<&str>) -> bool {
    p.is_some_and(|p| {
        let p = p.to_lowercase();
        p.ends_with(".cpp") || p.ends_with(".cc") || p.ends_with(".cxx")
    })
}

pub(super) fn combined_output_error(r: &TaskResult) -> String {
    format!(
        "{}\n{}",
        r.output.as_deref().unwrap_or(""),
        r.error.as_deref().unwrap_or("")
    )
}

/// 用户任务是否像「写 C++ 程序并编译运行」类请求（用于任务级验收门控）。
pub(super) fn is_program_build_run_request(task: &str) -> bool {
    let t = task.to_lowercase();
    let asks_write = t.contains("编写") || t.contains("实现") || t.contains("write");
    let asks_program = t.contains("程序") || t.contains("c++") || t.contains("cpp");
    let asks_run = t.contains("执行")
        || t.contains("运行")
        || t.contains("编译")
        || t.contains("build")
        || t.contains("run");
    asks_write && asks_program && asks_run
}

fn expected_output_hints_from_task(task: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(re) = regex::Regex::new(r#""([^"\n]{1,120})""#) {
        for cap in re.captures_iter(task) {
            if let Some(m) = cap.get(1) {
                let t = m.as_str().trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
            }
        }
    }
    if out.is_empty() && task.to_lowercase().contains("hello") {
        out.push("hello".to_string());
    }
    out
}

pub(super) fn expected_output_hints_for_results(
    task: &str,
    results: &[TaskResult],
    goal_expected_outputs: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in results {
        if let Some(v) = goal_expected_outputs.get(&r.task_id) {
            for s in v {
                let t = s.trim();
                if !t.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                    out.push(t.to_string());
                }
            }
        }
    }
    if out.is_empty() {
        return expected_output_hints_from_task(task);
    }
    out
}

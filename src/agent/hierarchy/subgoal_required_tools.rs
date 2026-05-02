//! 子目标描述与 `required_tools` 的启发式补全（与执行层并行 spawn 共用）。

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
    if mentions_compile_or_build_inspect(&d) {
        push("run_command");
    }
    if mentions_run(&d) {
        push("run_command");
        push("run_executable");
    }
}

fn mentions_compile_or_build_inspect(d: &str) -> bool {
    d.contains("编译")
        || d.contains("构建")
        || d.contains("--build")
        || d.contains("链接")
        || d.contains("make")
        || d.contains("g++")
        || d.contains("clang")
        || d.contains("ninja")
        || d.contains("meson")
        || (d.contains("cmake") && cmake_context_implies_run_command(d))
        || inspects_build_tree(d)
}

fn cmake_context_implies_run_command(d: &str) -> bool {
    d.contains("执行") || d.contains("配置") || d.contains("生成") || d.contains("安装")
}

fn inspects_build_tree(d: &str) -> bool {
    (d.contains("检查") || d.contains("确认") || d.contains("验证"))
        && (d.contains("build") || d.contains("可执行") || d.contains("产物") || d.contains("生成"))
}

fn mentions_run(d: &str) -> bool {
    d.contains("运行")
        || d.contains("执行")
        || d.contains("跑")
        || d.contains("验证输出")
        || d.contains("退出码")
        || d.contains("hello")
}

#[cfg(test)]
mod tests {
    use super::*;

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

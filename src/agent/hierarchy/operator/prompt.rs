//! ReAct 系统提示与按任务类型的执行指导文案。

use super::super::task::SubGoal;
use super::execution_guides;
use super::types::OperatorConfig;

pub(crate) fn build_system_prompt(
    config: &OperatorConfig,
    goal: &SubGoal,
    current_dir: Option<&std::path::Path>,
) -> String {
    let tools_list = if config.allowed_tools.is_empty() {
        "所有可用工具".to_string()
    } else {
        config.allowed_tools.join(", ")
    };

    let execution_guide = build_execution_guide(goal);

    let working_dir_info = current_dir
        .map(|d| format!("\n## 当前工作目录\n{}\n", d.display()))
        .unwrap_or_default();

    format!(
        r#"你是一个 ReAct (Reasoning + Acting) 代理。

当前任务：{}{}

## 可用工具
{}

## 执行指导
{}

## 规则
1. 首先分析任务，确定需要的工具
2. 每次只调用一个工具
3. 根据工具返回结果决定下一步
4. 任务完成后给出总结（包含"完成"或"finished"字样）

## 重要约束
- **禁止假设**任何文件或目录存在。调用 `read_dir`、`search_replace`、`modify_file` 等工具前，**必须先用 `read_dir` 确认目标路径存在**
- 如果工具返回"路径无法解析"或"No such file or directory"，**必须承认路径不存在**，不能再用相同的错误路径继续操作
- 如果不确定某个路径是否存在，先用 `read_dir` 的父目录来确认
- **创建文件必须使用 `create_file` 工具**，禁止使用 `echo`、`cat`、`tee` 等命令通过 `run_command` 创建文件
- `create_file` 的 `content` 参数：在 JSON 中必须使用正确的转义序列，换行用 `\n`，制表用 `\t`，双引号用 `\"

## 工作目录管理（关键！）
- **当前工作目录**已在上方"当前工作目录"部分显示
- **所有相对路径都是基于当前工作目录的**
- 如果需要在子目录中执行命令，有三种方式（按推荐顺序）：
  1. **使用 `-C` 选项**（推荐）：`make -C subdirectory`
  2. **使用完整路径作为 command**：`command: "subdirectory/script.sh"`, `args: []`
  3. **使用完整路径在 args 中**：`command: "cp"`, `args: ["subdirectory/src", "subdirectory/dest"]`
- **禁止**使用 `cd` 命令后再执行其他命令（cd 不会持久化工作目录）
- **常见错误示例**：
  - ❌ 错误：`command: "./configure"`, `args: ["subdirectory/configure"]` —— command 和 args 重复了路径
  - ❌ 错误：`command: "cp"`, `args: ["setup/file", "dest"]` —— 当前目录下没有 setup/ 目录
  - ✅ 正确：`command: "cp"`, `args: ["subdirectory/setup/file", "subdirectory/dest"]` —— 使用完整路径
  - ✅ 正确：`command: "subdirectory/configure"`, `args: []` —— 完整路径作为 command
- 如果命令返回"找不到文件"或"No such file or directory"，首先检查工作目录是否正确
"#,
        goal.description, working_dir_info, tools_list, execution_guide
    )
}

/// 根据目标类型构建执行指导（关键词路由见 `execution_guides`）。
fn build_execution_guide(goal: &SubGoal) -> String {
    let desc = goal.description.to_lowercase();
    if execution_guides::is_build_task(&desc) {
        return execution_guides::BUILD.to_string();
    }
    if execution_guides::is_test_task(&desc) {
        return execution_guides::TEST.to_string();
    }
    if execution_guides::is_debug_task(&desc) {
        return execution_guides::DEBUG.to_string();
    }
    if execution_guides::is_deploy_task(&desc) {
        return execution_guides::DEPLOY.to_string();
    }
    if execution_guides::is_review_task(&desc) {
        return execution_guides::REVIEW.to_string();
    }
    if execution_guides::is_deps_task(&desc) {
        return execution_guides::DEPS.to_string();
    }
    if execution_guides::is_check_tools_task(&desc) {
        return execution_guides::CHECK_TOOLS.to_string();
    }
    if execution_guides::is_file_ops_task(&desc) {
        return execution_guides::FILE_OPS.to_string();
    }
    execution_guides::DEFAULT.to_string()
}

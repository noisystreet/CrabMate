//! Manager 侧提示词拼装与工作区/工具 schema 上下文。

use super::super::task::{Artifact, SubGoal, TaskResult, TaskStatus};
use super::types::{ManagerAgent, ManagerDecision, ManagerError};

impl ManagerAgent {
    pub(super) fn build_failed_goal_prompt(
        &self,
        failed_goal: &SubGoal,
        error_message: &str,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        previous_artifacts: &[Artifact],
    ) -> String {
        let workspace_context = self.get_workspace_context(working_dir);
        let artifacts_summary = self.format_artifacts_summary(previous_artifacts);

        format!(
            r#"## 任务
你是一个任务执行协调专家。子目标执行失败，需要你决定如何处理。

原始子目标：{}
目标描述：{}

执行失败信息：
{}

## 当前工作目录
{}
重要：基于实际文件状态做决策。
**禁止假设**任何文件或目录存在，必须先通过 read_dir 确认。

## 已有的产物（可供参考）
{}

## 工具定义（完整参数 schema）
{}
重要：
{}

## 决策要求

分析失败原因，从以下选项中选择：

1. **重试（Retry）**：如果失败是因为工具参数错误或描述不准确，返回修改后的子目标。
   - JSON 格式：{{"decision": "retry", "updated_goal": {{"goal_id": "goal_1", "description": "修改后的描述", "priority": 1, "depends_on": [], "required_tools": ["tool1"]}}}}

2. **跳过（Skip）**：如果失败是因为条件不满足或无法完成，标记跳过并提供原因。
   - JSON 格式：{{"decision": "skip", "reason": "跳过原因"}}

3. **终止（Abort）**：如果任务根本无法完成，终止整个任务。
   - JSON 格式：{{"decision": "abort", "reason": "终止原因"}}

## 输出格式
只输出 JSON，不要有任何解释文字。
"#,
            failed_goal.goal_id,
            failed_goal.description,
            error_message,
            workspace_context,
            artifacts_summary,
            self.format_tools_with_schemas(tools_defs),
            Self::manager_tool_invariants(),
        )
    }

    /// 解析失败决策
    pub(super) fn parse_failure_decision(
        &self,
        content: &str,
        original_goal: &SubGoal,
    ) -> Result<ManagerDecision, ManagerError> {
        let json_str =
            super::super::manager_json_repair::extract_json(content).ok_or_else(|| {
                ManagerError::ParseError("Failed to extract JSON from response".to_string())
            })?;

        #[derive(serde::Deserialize)]
        struct DecisionJson {
            decision: String,
            updated_goal: Option<SubGoalJson>,
            reason: Option<String>,
        }

        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct SubGoalJson {
            goal_id: String,
            description: String,
            priority: Option<u32>,
            depends_on: Option<Vec<String>>,
            #[serde(default)]
            consumes_from_dependencies: Option<Vec<super::super::task::DependencyContractEntry>>,
            required_tools: Option<Vec<String>>,
            #[serde(default)]
            goal_type: Option<super::super::task::GoalType>,
        }

        let parsed: DecisionJson =
            serde_json::from_str(json_str).map_err(|e| ManagerError::ParseError(e.to_string()))?;

        match parsed.decision.as_str() {
            "retry" => {
                if let Some(ug) = parsed.updated_goal {
                    Ok(ManagerDecision::Retry {
                        updated_goal: Box::new(SubGoal {
                            goal_id: ug.goal_id,
                            description: ug.description,
                            priority: ug.priority.unwrap_or(original_goal.priority),
                            depends_on: ug.depends_on.unwrap_or_default(),
                            consumes_from_dependencies: ug
                                .consumes_from_dependencies
                                .unwrap_or_default(),
                            required_tools: ug.required_tools.unwrap_or_default(),
                            goal_type: original_goal.goal_type.clone(),
                            build_requirements: original_goal.build_requirements.clone(),
                            acceptance: None,
                            max_retries: None,
                        }),
                    })
                } else {
                    Ok(ManagerDecision::Retry {
                        updated_goal: Box::new(original_goal.clone()),
                    })
                }
            }
            "skip" => Ok(ManagerDecision::Skip {
                reason: parsed
                    .reason
                    .unwrap_or_else(|| "Skipped by manager".to_string()),
            }),
            "abort" => Ok(ManagerDecision::Abort {
                reason: parsed
                    .reason
                    .unwrap_or_else(|| "Aborted by manager".to_string()),
            }),
            _ => Ok(ManagerDecision::Skip {
                reason: format!("Unknown decision: {}", parsed.decision),
            }),
        }
    }

    /// 构建重新规划的 prompt
    pub(super) fn build_replan_prompt(
        &self,
        original_task: &str,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
        previous_results: &[TaskResult],
        previous_artifacts: &[Artifact],
    ) -> String {
        let workspace_context = self.get_workspace_context(working_dir);
        let tools_description = self.format_tools_with_schemas(tools_defs);

        // 生成已完成 artifacts 的摘要
        let artifacts_summary = self.format_artifacts_summary(previous_artifacts);

        // 生成失败信息摘要
        let failures_summary = self.format_failures_summary(previous_results);

        format!(
            r#"## 任务
你是一个任务分解专家。原始任务需要重新规划。

原始任务：{}

## 分解硬性规则（必须遵守）
{}

## 工作目录上下文
{}
重要：
- 子目标的描述必须基于**实际存在的**文件和目录
- **禁止假设**任何文件或目录存在，必须先通过 read_dir 确认
- **禁止在子目标描述中使用不存在的路径**，如 `src/`、`include/`、`build/` 等，除非已确认存在
- 如果需要读取某个目录，**必须先用 read_dir 确认它存在**，才能在后续子目标中使用
- 如果需要操作某个文件（如 search_replace、modify_file），**必须先确认文件存在**

## 已完成的产物（可供后续子目标使用）
{}
重要：后续子目标应该引用这些已创建的文件/产物，而不是重复创建。

## 失败信息
{}
重要：分析失败原因，在重新规划时避免相同的问题。

## 工具定义（完整参数 schema）
{}
重要：
{}

## CMake 项目特殊规则
- 如果工作目录包含 CMakeLists.txt，**必须使用**其中定义的可执行文件目标名称
- **禁止假设**可执行文件名称（如 demo、test、main 等），必须使用 CMakeLists.txt 中 `add_executable()` 定义的实际名称
- **禁止**在源码树根部使用 `file(GLOB_RECURSE "*.cpp" …)` 且不排除 `build/`、`CMakeFiles/`：会把 CMake 生成的 `CompilerId*.c/cpp` 等编进同一可执行目标，链接报 **multiple definition of `main`**。简单项目请 **`add_executable(目标名 main.cpp)`** 显式列源；若必须用 GLOB，须**排除** `build` 与 `CMakeFiles` 目录
- 运行构建产物时，**优先**用 **`run_executable` + 工作区相对路径**（如经 `read_dir` 在 `build/` 中确认后的 `build/<目标名>`）；不要用猜测的名称或错误的 JSON `args` 等「凑合」方式跳过验证
- 凡子目标描述含 **cmake / 编译 / make / 构建 / 检查 build / 验证可执行** 等，且你填写了非空 `required_tools`，**必须**包含 **`run_command`**（以及需要直接跑产物时的 **`run_executable`**）；仅 `read_dir` 会导致无法执行 `cmake --build` 等而空转

## Cargo/Rust 项目特殊规则
- 如果执行了 `cargo init` 且创建了子目录（如 `tmp/`），后续所有 `cargo` 命令必须在那个子目录中执行
- 使用 `run_command` 执行 cargo 命令前，**必须先 `cd` 到项目目录**：`{{"command": "cd", "args": ["tmp"]}}`，然后再执行 `{{"command": "cargo", "args": ["build"]}}`
- **禁止假设**可执行文件名称，必须使用 `Cargo.toml` 中 `[[bin]]` 或默认的 `src/main.rs` 对应的名称
- 运行 Rust 可执行文件时，路径必须是 `./target/debug/<名称>`（在子目录内）或 `./tmp/target/debug/<名称>`（从根目录）

## 子目标 I/O 契约
- 同「初次分解」：每个子目标 `description` 写清 I/O；`depends_on` + `consumes_from_dependencies` + 可选 `build_requirements`；在描述与工具里用 `{{ref:<前序id>:<artifact_id>}}` 或 `{{artifact:...}}`，**不要**写绝对路径。

## 输出格式
**必须输出标准 JSON 格式**，不要输出任何其他内容。JSON 必须符合以下结构：
```
{{{{
    "sub_goals": [
        {{{{
            "goal_id": "goal_1",
            "description": "（I/O 契约）子目标描述（基于实际文件结构和已有产物）",
            "priority": 1,
            "depends_on": ["goal_id_of_dependency"],
            "consumes_from_dependencies": [
                {{"from_goal_id": "goal_id_of_dependency", "only_kinds": null}}
            ],
            "build_requirements": {{"needs_artifacts": [], "produces_artifacts": []}},
            "required_tools": ["tool_name1", "tool_name2"],
            "goal_type": "fix"  // 或 "analyze"
        }}}}
    ],
    "execution_strategy": "hybrid"
}}}}
```
- `goal_id` 必须是字符串
- `description` 必须是字符串
- `priority` 必须是数字
- `depends_on` 必须是字符串数组
- `consumes_from_dependencies` 可选，规则同初次分解
- `build_requirements` 可选
- `required_tools` 必须是字符串数组
- `goal_type` 必须是 `"fix"`（修复/执行）或 `"analyze"`（分析/收集）。如果只需要收集信息（如编译错误），用 `"analyze"`，失败后直接跳过。

## 强约束 Schema（必须满足）
```json
{}
```

## 约束
- 子目标数量不超过 {}
- **只输出 JSON，不要有markdown代码块标记、不要有任何解释文字**
- 尽量复用已有的 artifacts，避免重复创建相同的文件
"#,
            original_task,
            Self::DECOMPOSITION_RULES_1_TO_10,
            workspace_context,
            artifacts_summary,
            failures_summary,
            tools_description,
            Self::manager_tool_invariants(),
            Self::manager_output_schema_contract(),
            self.config.max_sub_goals
        )
    }

    /// 格式化 artifacts 摘要
    pub(super) fn format_artifacts_summary(&self, artifacts: &[Artifact]) -> String {
        if artifacts.is_empty() {
            return "(尚无产物)".to_string();
        }

        let mut lines = Vec::new();
        for artifact in artifacts {
            let path_info = artifact
                .path
                .as_ref()
                .map(|p| format!(" (路径: {})", p))
                .unwrap_or_default();
            lines.push(format!(
                "- [{}] {}{}",
                format!("{:?}", artifact.kind).to_lowercase(),
                artifact.name,
                path_info
            ));
        }
        lines.join("\n")
    }

    /// 格式化失败摘要
    pub(super) fn format_failures_summary(&self, results: &[TaskResult]) -> String {
        let failures: Vec<_> = results
            .iter()
            .filter(|r| matches!(r.status, TaskStatus::Failed { .. }))
            .collect();

        if failures.is_empty() {
            return "(无失败)".to_string();
        }

        let mut lines = Vec::new();
        for result in failures {
            let reason = match &result.status {
                TaskStatus::Failed { reason } => reason.clone(),
                _ => unreachable!(),
            };
            lines.push(format!("- 子目标 {} 失败: {}", result.task_id, reason));
        }
        lines.join("\n")
    }

    /// 构建分解 prompt
    pub(super) fn build_decomposition_prompt(
        &self,
        task: &str,
        working_dir: &std::path::Path,
        tools_defs: &[crate::types::Tool],
    ) -> String {
        // 获取工作目录上下文
        let workspace_context = self.get_workspace_context(working_dir);

        // 生成完整工具定义（包含参数 schema）
        let tools_description = self.format_tools_with_schemas(tools_defs);

        // 识别任务类型并添加特定指导
        let task_type_guidance = self.get_task_type_guidance(task);

        format!(
            r#"## 任务
你是一个任务分解专家。请将以下用户任务分解为可执行的子目标。

任务：{}

## 任务类型识别与指导
{}

## 分解硬性规则（必须遵守）
{}

## 子目标 I/O 契约（必须显式写清，便于层间传产物与注入裁剪）
- 对**每个**子目标在 `description` 开头用 2～4 行写清：本步**输入/依赖**、本步**预期输出**（路径或行为）；若依赖前序子目标，须写进 `depends_on`。
- `consumes_from_dependencies`：列出本步**实际消费**的前序 `goal_id`；`only_kinds` 可选，用于**注入到 Operator 的依赖上下文**的裁剪：省略或 `[]` 表示**默认**（不注入冗长 `buildlog` 与纯长文本 `commandoutput`）；填 `["all"]` 或 `["any"]` 表示不筛类型；否则为子串匹配，与产物类型字符串（如 `buildartifact(executable)`、`file`）不区分大小写子串匹配，例如 `["source"]`、`["executable"]`。
- `build_requirements` 可选；编译类任务可填 `needs_artifacts` / `produces_artifacts`（`SourceFile` / `ObjectFile` / `Executable` 等）。
- 在 `description` 与工具设计里，引用前序**具体文件/构建物**时优先写 **`{{ref:<前序子目标id>:<artifact_id>}}`** 或 `{{artifact:文件名.stem}}`；**不要**在 JSON 中写本机**绝对**路径。执行时 `{{ref:...}}` 会展开为工作区相对 path。

## 工作目录上下文
{}
重要：
- 子目标的描述必须基于**实际存在的**文件和目录
- **禁止假设**任何文件或目录存在，必须先通过 read_dir 确认
- **禁止在子目标描述中使用不存在的路径**，如 `src/`、`include/`、`build/` 等，除非已确认存在
- 如果需要读取某个目录，**必须先用 read_dir 确认它存在**，才能在后续子目标中使用
- 如果需要操作某个文件（如 search_replace、modify_file），**必须先确认文件存在**

## 工具定义（完整参数 schema）
{}
重要：
{}

## 输出格式
**必须输出标准 JSON 格式**，不要输出任何其他内容。JSON 必须符合以下结构：
```{{{{
    "sub_goals": [
        {{{{
            "goal_id": "goal_1",
            "description": "（I/O: 输入/输出/产物）子目标描述（基于实际文件结构）",
            "priority": 1,
            "depends_on": ["goal_0"],
            "consumes_from_dependencies": [
                {{"from_goal_id": "goal_0", "only_kinds": null}}
            ],
            "build_requirements": {{"needs_artifacts": ["SourceFile"], "produces_artifacts": ["Executable"]}},
            "required_tools": ["tool_name1", "tool_name2"],
            "goal_type": "fix"  // 或 "analyze"
        }}}}
    ],
    "execution_strategy": "hybrid"
}}}}
```
- `goal_id` 必须是字符串
- `description` 必须是字符串
- `priority` 必须是数字
- `depends_on` 必须是字符串数组
- `consumes_from_dependencies` 是可选数组，每项为 `from_goal_id` 字符串 + 可选 `only_kinds: string[] | null`（`from_goal_id` 必须出现在 `depends_on` 中；空数组可省略本字段，由执行器在可行时**自动补全**并默认裁剪类型）
- `build_requirements` 可选
- `required_tools` 必须是字符串数组
- `goal_type` 必须是 `"fix"`（修复/执行）或 `"analyze"`（分析/收集）。如果只需要收集信息（如编译错误），用 `"analyze"`，失败后直接跳过。

## 强约束 Schema（必须满足）
```json
{}
```

## 约束
- 子目标数量不超过 {}
- **只输出 JSON，不要有markdown代码块标记、不要有任何解释文字**
"#,
            task,
            task_type_guidance,
            Self::DECOMPOSITION_RULES_1_TO_10,
            workspace_context,
            tools_description,
            Self::manager_tool_invariants(),
            Self::manager_output_schema_contract(),
            self.config.max_sub_goals
        )
    }

    /// 根据任务描述识别任务类型并返回特定指导
    pub(super) fn get_task_type_guidance(&self, task: &str) -> String {
        let task_lower = task.to_lowercase();

        // 编译类任务
        if task_lower.contains("编译")
            || task_lower.contains("build")
            || task_lower.contains("make")
        {
            return r#"**识别为：编译/构建任务**

用户意图：将源代码编译为可执行文件或库

**必须分解的完整步骤**：
1. **确认源码存在** - 检查压缩包或源码目录
2. **解压源码**（如果是压缩包）- 使用 archive_unpack
3. **查找和阅读文档** - 查找 README、INSTALL、BUILDING、docs/ 等文档，了解构建要求和步骤
4. **检查构建系统** - 查看 Makefile/CMakeLists.txt/configure 等
5. **检查编译工具** - 确认 gcc/g++/make/cmake 等存在
6. **执行编译** - 运行 make/cmake 等构建命令
7. **验证产物** - 用 `read_dir` 等检查构建输出目录中是否出现可执行文件/预期目标
8. **运行并核对** - 用 **`run_executable`** 等工作区内运行能力执行产物、核对退出码与（如有）标准输出；用户若要求可运行验收、演示或「能跑起来」，**本步与前面步骤同等重要**，子目标**不得**停在仅编译通过

**CMake 编写要点**（模型生成 `CMakeLists.txt` 时）：
- 单文件示例程序用 **`add_executable(目标 main.cpp)`**，避免根目录 **`file(GLOB_RECURSE "*.cpp")`** 把 `build/CMakeFiles/**/CMake*CompilerId.*` 编进目标引发链接错误

**重要**：
- 不要只分解"检查"步骤，必须包含完整的编译与（若适用）**运行**流程！
- **务必先阅读文档** - 很多项目有特定的构建要求和依赖，文档中会说明正确的构建步骤
"#
            .to_string();
        }

        // 代码修改类任务
        if task_lower.contains("修改") || task_lower.contains("修复") || task_lower.contains("fix")
        {
            return r#"**识别为：代码修改/修复任务**

用户意图：修改代码文件以修复问题或实现功能

**必须分解的完整步骤**：
1. **定位目标文件** - 找到需要修改的文件
2. **读取当前内容** - 使用 read_file 查看文件
3. **执行修改** - 使用 search_replace 或 modify_file
4. **验证修改** - 读取文件确认修改成功
5. **测试**（如需要）- 运行相关测试验证修复

**重要**：不要只分解"查找"步骤，必须包含实际的修改操作！
"#
            .to_string();
        }

        // 分析/调查类任务
        if task_lower.contains("分析") || task_lower.contains("查看") || task_lower.contains("调查")
        {
            return r#"**识别为：分析/调查任务**

用户意图：收集信息、分析问题或查看状态

**分解要点**：
- 明确需要收集哪些信息
- 确定信息来源（日志文件、配置文件、目录结构等）
- 如果需要多步骤分析，确保步骤之间有逻辑关联

**重要**：分析任务应该产出明确的结论或报告！
"#
            .to_string();
        }

        // 默认指导
        r#"**通用任务**

请确保：
1. 完整理解用户意图 - 不要只分解验证/检查步骤
2. 子目标应该覆盖任务的完整生命周期
3. 如果任务涉及多个阶段（准备→执行→验证），确保每个阶段都有对应的子目标
"#
        .to_string()
    }

    /// 分解、重试、重规划、反思、失败处理等阶段共用的**工具/JSON 固定规范**（写入 Manager 提示，保证子目标与 Executor 调用工具一致）。
    pub(super) fn manager_tool_invariants() -> &'static str {
        r#"- 分配工具时，确保参数与子目标、工具 `parameters` 一致
- **`create_file` 是向工作区新建普通文件的正确方式**；禁止用 `echo`/`cat`/`tee` 经 `run_command` 建文件
- `path` 优先**相对工作区根**（如 `main.cpp`）；**勿**在子目标或工具参数里造深层误路径（如无关子目录下的 `main.cpp`）
- `create_file` 仅当目标路径**尚不存在**时成功；已存在时须用 `modify_file`、`search_replace`、`append_file` 等，**禁止**对同一路径重复 `create_file`
- `run_command` 须分别传 `command` 与 `args`；`args` 在 JSON 中必须是**字符串**数组。每一项**必须**用双引号包起来（如列表标志为 `\"-la\"` 的数组元素，不得写成无引号 token），否则易触发「参数解析错误」或 `invalid number`
- 命令健康检查模板化：对编译工具可用性按“定位 + 版本”两步分别执行，避免混写假失败：
  - `which cmake` 后再 `cmake --version`
  - `which g++` 后再 `g++ --version`
  - 对应 `run_command` JSON 形态分别为：
    - `{ \"command\": \"which\", \"args\": [\"cmake\"] }`
    - `{ \"command\": \"cmake\", \"args\": [\"--version\"] }`
    - `{ \"command\": \"which\", \"args\": [\"g++\"] }`
    - `{ \"command\": \"g++\", \"args\": [\"--version\"] }`
  - **禁止**写成 `\"command\": \"which cmake\"` 或把 `which` 与 `--version` 混在一次调用里（例如 `which cmake --version`）
- 简单 CMake 项目用 **`add_executable(… main.cpp)`** 等**显式列出源文件**；勿对**空** `file(GLOB …)` 结果生成目标（会无源可链）；**勿**用未排除 `build/` 的 **`GLOB_RECURSE`** 收集 `*.cpp`（会把 `CMakeFiles/` 下探测源链进来导致重复 `main`）
- 工作区内的**可执行/构建产物**的「运行（执行）」优先用 **`run_executable` + 相对工作区根路径**；白名单**系统**命令用 `run_command`；以工具说明与 `config` 中分工为准
- 子目标若属于「运行可执行体 / 验证程序输出」：`required_tools` **必须包含** `run_executable`，并以 `run_executable` 为主执行；`run_command` 仅作补充诊断，不得替代主验证
- 子目标若属于「编译构建」：描述与步骤中**禁止**包含运行可执行体动作；运行与输出核对应拆到独立后续子目标
- 须**从源码到可跑通、输出可核对**的完整类任务，子目标**必须**含**运行产物并验证**的一步；**不得**在「只编译/只生成文件」时视为整任务完成
- `read_dir` 路径为不含 `..` 的相对路径
- `create_file` 的 `content` 为 JSON 字符串，须按规范对换行、引号等转义"#
    }

    /// 获取工作目录上下文信息
    pub(super) fn get_workspace_context(&self, working_dir: &std::path::Path) -> String {
        let dir_path = working_dir.display();

        // 列出目录内容
        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(working_dir) {
            for entry in read_dir.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    let path = entry.path();
                    let is_dir = path.is_dir();
                    let prefix = if is_dir { "[DIR] " } else { "[FILE]" };
                    entries.push(format!("{} {}", prefix, name));
                }
            }
        }

        let entries_str = if entries.is_empty() {
            "(目录为空或无法读取)".to_string()
        } else {
            entries.join("\n")
        };

        // 检查是否有 build 目录
        let build_info = if working_dir.join("build").is_dir() {
            "\n注意：存在 build/ 目录（CMake 构建产物可能在这里）".to_string()
        } else {
            String::new()
        };

        // 检查是否有 src 目录
        let src_info = if working_dir.join("src").is_dir() {
            "\n注意：存在 src/ 目录".to_string()
        } else {
            String::new()
        };

        // 解析 CMakeLists.txt 获取可执行文件名称
        let cmake_info = self.parse_cmake_info(working_dir);

        format!(
            r#"当前工作目录：{}
目录内容：
{}{}{}{}{}"#,
            dir_path,
            entries_str,
            build_info,
            src_info,
            cmake_info,
            if entries.len() > 20 {
                "\n(只显示前20项)"
            } else {
                ""
            }
        )
    }

    /// 解析 CMakeLists.txt 获取项目信息
    pub(super) fn parse_cmake_info(&self, working_dir: &std::path::Path) -> String {
        let cmake_path = working_dir.join("CMakeLists.txt");
        if !cmake_path.exists() {
            return String::new();
        }

        let content = match std::fs::read_to_string(&cmake_path) {
            Ok(c) => c,
            Err(_) => return String::new(),
        };

        let mut info_parts = Vec::new();

        // 解析项目名称
        if let Some(project_name) = self.extract_cmake_project_name(&content) {
            info_parts.push(format!("CMake 项目名称: {}", project_name));
        }

        // 解析可执行文件目标
        let executables = self.extract_cmake_executables(&content);
        if !executables.is_empty() {
            info_parts.push(format!("CMake 可执行文件目标: {}", executables.join(", ")));
        }

        if info_parts.is_empty() {
            String::new()
        } else {
            format!("\nCMake 项目信息:\n  - {}", info_parts.join("\n  - "))
        }
    }

    /// 从 CMakeLists.txt 内容中提取项目名称
    pub(super) fn extract_cmake_project_name(&self, content: &str) -> Option<String> {
        // 匹配 project(Name) 或 project(Name VERSION x.y.z)
        let re = regex::Regex::new(r"project\s*\(\s*([A-Za-z0-9_]+)").ok()?;
        re.captures(content)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
    }

    /// 从 CMakeLists.txt 内容中提取可执行文件目标
    pub(super) fn extract_cmake_executables(&self, content: &str) -> Vec<String> {
        let mut executables = Vec::new();

        // 匹配 add_executable(name source1 source2 ...)
        let re = regex::Regex::new(r"add_executable\s*\(\s*([A-Za-z0-9_]+)").ok();
        if let Some(re) = re {
            for cap in re.captures_iter(content) {
                if let Some(name) = cap.get(1) {
                    executables.push(name.as_str().to_string());
                }
            }
        }

        executables
    }

    /// 格式化工具定义，包含完整参数 schema
    pub(super) fn format_tools_with_schemas(&self, tools_defs: &[crate::types::Tool]) -> String {
        tools_defs
            .iter()
            .map(|t| {
                let name = &t.function.name;
                let description = &t.function.description;
                let params = &t.function.parameters;

                // 提取 parameters properties 作为参数说明
                let params_desc = if let Some(props) = params.get("properties") {
                    if let Some(obj) = props.as_object() {
                        obj.iter()
                            .map(|(param_name, param_info)| {
                                // 获取参数类型描述
                                let param_type = param_info
                                    .get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("any");
                                let param_desc = param_info
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let enum_vals = param_info.get("enum").and_then(|v| {
                                    v.as_array().map(|arr| {
                                        arr.iter()
                                            .filter_map(|x| x.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    })
                                });
                                let enum_str = enum_vals
                                    .map(|e| format!(" (可选值: {})", e))
                                    .unwrap_or_default();
                                format!(
                                    "  - {}: {}（类型：{}{}）",
                                    param_name, param_desc, param_type, enum_str
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                if params_desc.is_empty() {
                    format!("### {}\n{}\n(无参数)", name, description)
                } else {
                    format!("### {}\n{}\n参数：\n{}", name, description, params_desc)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

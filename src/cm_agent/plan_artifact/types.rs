use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use crate::cm_agent::text_markers::text_matches_any;

/// 规划步骤 `id` / 可选 `workflow_node_id` 的语法：稳定、可日志引用，并与工作流节点 `id` 常见字符集对齐。
static PLAN_STEP_ID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9][-A-Za-z0-9_./]{0,127}$")
        .expect("PLAN_STEP_ID_PATTERN: 编译期正则须合法")
});

pub fn plan_step_id_syntax_ok(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.len() <= 128 && PLAN_STEP_ID_PATTERN.is_match(t)
}

/// 约定的规划 JSON：`type` + `version` + `steps`；若 `no_task` 为 true 则表示无具体可拆任务，`steps` 须为空。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AgentReplyPlanV1 {
    #[serde(rename = "type")]
    pub plan_type: String,
    pub version: u32,
    pub steps: Vec<PlanStepV1>,
    /// 为 true：模型判定用户未提出需分步执行的具体任务；此时 `steps` 必须为空。
    #[serde(default)]
    pub no_task: bool,
}

/// 分阶段规划单步「子代理」角色：收窄该步内外层循环可见的 **OpenAI tools 列表**，并在执行层拒绝越权 `tool_calls`（与 `write_effect_tools` / 只读判定一致）。
/// 省略或 `null` 表示不限制（与历史行为一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepExecutorKind {
    /// 仅允许语义只读工具（`is_readonly_tool`）；禁止 MCP 代理工具。
    /// 同时是 `Default`（`cm_workflow` 侧的旧副本以此为默认，`#1247` 合并时保留该语义）。
    #[default]
    ReviewReadonly,
    /// 只读工具 + 受限写补丁类（`apply_patch` / `search_replace` / `structured`（其 `action=patch` 是写侧，合并后整工具按写处理）/ `create_file` / `modify_file` / `append_file`）。
    PatchWrite,
    /// 只读工具 + 常见测试运行器（如 `cargo_test` / `pytest_run` 等）；**不含**任意 `run_command`。
    TestRunner,
}

impl PlanStepExecutorKind {
    /// 与规划 JSON / SSE 中 `executor_kind` 字符串一致（蛇形）。
    pub fn as_snake_case_str(self) -> &'static str {
        match self {
            PlanStepExecutorKind::ReviewReadonly => "review_readonly",
            PlanStepExecutorKind::PatchWrite => "patch_write",
            PlanStepExecutorKind::TestRunner => "test_runner",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct PlanStepAcceptance {
    /// 期望的退出码（如 `cargo test` → 0）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_exit_code: Option<i32>,
    /// 期望 stdout 包含的字符串。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_stdout_contains: Option<String>,
    /// 期望 stderr 包含的字符串。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_stderr_contains: Option<String>,
    /// 期望存在的文件路径（相对于工作区）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_file_exists: Option<String>,
    /// JSON path 验证：期望该路径的值等于指定值。
    /// - **Legacy**：`$.field.nested`、`$[0].field`、`$.items[0][1]`（段内可多段 `[n]`）；空白路径表示整份 JSON。
    /// - **RFC 6901 JSON Pointer**：以 `/` 开头，例如 `/a/b/0`；`/` 表示键名为空字符串；键内含 `/` 或 `~` 时用 `~1`、`~0` 转义。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_json_path_equals: Option<JsonPathEqualsRule>,
    /// HTTP 状态码验证：期望的 HTTP 状态码。
    /// 仅对 `http_request` / `http_fetch` 类工具生效。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_http_status: Option<u16>,
}

impl PlanStepAcceptance {
    /// 是否含至少一条可执行的验收规则（空对象 `{}` 为 false）。
    pub fn is_effective(&self) -> bool {
        self.expect_exit_code.is_some()
            || self
                .expect_stdout_contains
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
            || self
                .expect_stderr_contains
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
            || self
                .expect_file_exists
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
            || self
                .expect_json_path_equals
                .as_ref()
                .is_some_and(|r| !r.path.trim().is_empty())
            || self.expect_http_status.is_some()
    }

    /// 补丁规划 **user** 用：列出非空验收字段作为「参考 r」，子串/路径截断以免撑爆上下文。
    pub fn compact_reference_for_planner_feedback(&self) -> Option<String> {
        const SUB_PREVIEW: usize = 48;
        const PATH_PREVIEW: usize = 64;
        let mut parts: Vec<String> = Vec::new();
        if let Some(c) = self.expect_exit_code {
            parts.push(format!("expect_exit_code={c}"));
        }
        push_previewed_field(
            &mut parts,
            "expect_stdout_contains",
            self.expect_stdout_contains.as_deref(),
            SUB_PREVIEW,
        );
        push_previewed_field(
            &mut parts,
            "expect_stderr_contains",
            self.expect_stderr_contains.as_deref(),
            SUB_PREVIEW,
        );
        push_previewed_field(
            &mut parts,
            "expect_file_exists",
            self.expect_file_exists.as_deref(),
            PATH_PREVIEW,
        );
        if let Some(ref rule) = self.expect_json_path_equals
            && !rule.path.trim().is_empty()
        {
            parts.push(format!(
                "expect_json_path_equals.path={}",
                crate::cm_agent::preview_chars(rule.path.trim(), PATH_PREVIEW)
            ));
        }
        if let Some(h) = self.expect_http_status {
            parts.push(format!("expect_http_status={h}"));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("；"))
        }
    }
}

fn push_previewed_field(parts: &mut Vec<String>, label: &str, value: Option<&str>, preview: usize) {
    let Some(s) = value.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    parts.push(format!(
        "{label}={}",
        crate::cm_agent::preview_chars(s, preview)
    ));
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct JsonPathEqualsRule {
    /// JSON path 表达式（Legacy `$…` 或 RFC 6901 Pointer `/…`，见 `expect_json_path_equals` 文档）。
    pub path: String,
    /// 期望的 JSON 值（支持任意 JSON 标量/对象/数组）。
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlanStepControlFlow {
    pub condition: String,
    pub target_step_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_loops: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlanStepV1 {
    pub id: String,
    pub description: String,
    /// 可选：对应最近一次 `workflow_validate_only` 结果中 `nodes[].id`，供机器校验与轨迹对齐。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_node_id: Option<String>,
    /// 可选：本步执行子循环的工具角色（子代理）；省略则全量工具。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_kind: Option<PlanStepExecutorKind>,
    /// 可选：本步骤的确定性验收条件。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<PlanStepAcceptance>,
    /// 可选：本步骤允许的最大重试次数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_step_retries: Option<u32>,
    /// 可选：状态机控制流，用于定义执行完毕后的跳转规则。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transitions: Option<Vec<PlanStepControlFlow>>,
}

/// 将无实质字段的 `acceptance`（如 `{}`）规范为 `None`，并按 `executor_kind` / 步描述剥离不匹配的验收字段。
pub fn normalize_plan_step_acceptance_in_place(step: &mut PlanStepV1) {
    align_plan_step_acceptance_with_executor_kind(step);
}

/// 步描述是否**命中**任一关键词（`text_markers` 的单一实现；#1239 前各抄一份）。
///
/// #1422 B1：英文标记按 token 边界匹配，中文保留子串语义。
/// 方向取「漏词即保守」：少命中 ⇒ 少触发「只说不做」纠偏，
/// 而不是把文案猜测做得更全。唯一例外是 `BUILD_EXECUTION_KEYWORDS` 里显式补回的动词变形。
fn plan_step_description_matches_any(desc: &str, keywords: &[&str]) -> bool {
    text_matches_any(&desc.to_lowercase(), keywords)
}

/// 编译 / 构建 / 测试执行阶段的关键词。
///
/// 走 token 边界后 `build` 不再命中 `rebuild` / `builds`、`compile` 不再命中 `recompile`。
/// 补偿判据**只**覆盖两族：`re-` 前缀（`rebuild` / `recompile`）与 `-ing`（`building` /
/// `compiling`）——它们是描述「这一步在做什么」时的高频形态，漏掉会让
/// `outer_loop_task_implies_build_execution` 少触发「只说不做」纠偏。
/// `-s` 形式（`builds`）**刻意不补**：任务描述多为祈使句，且漏词落在保守侧（少触发纠偏），
/// 不为此把表撑大。
const BUILD_EXECUTION_KEYWORDS: &[&str] = &[
    "编译",
    "构建",
    "build",
    "building",
    "rebuild",
    "compile",
    "compiling",
    "recompile",
    "make ",
    "cmake",
    "link",
    "test_runner",
    "运行测试",
    "跑测试",
    "cargo test",
    "cargo build",
    "pytest",
    "go test",
    "ninja",
    "meson",
];

/// 步描述是否像「编译/构建/测试执行」阶段（而非仅解压/阅读/分析）。
///
/// # 这是意图猜测，不是完备判据
///
/// 本函数靠**自然语言描述**反推执行语义，两头都不通用：词形态收不全（`build` / `builds` /
/// `built` / `rebuild`）、构建入口枚举不完（`just` / `zig build` / `dotnet build` / `gradle` …）。
/// 补词只能缩小偏差，**不能收敛——不要靠往表里加词来「修」漏判**。
///
/// 现存消费者只剩 `outer_loop_task_implies_build_execution`（`outer_loop_build_idle` 的
/// 「只说不做」纠偏）：输入只有用户任务原文，注定只能猜（#1422 非目标）。
/// `align_plan_step_acceptance_with_executor_kind` 已**不再**使用本表——它只看
/// `executor_kind` 结构化信号（#1427 第 3 步）；`default_exit_code_for_build_execution_description`
/// 已随 #1429 删除。
pub fn plan_step_description_implies_build_execution(desc: &str) -> bool {
    plan_step_description_matches_any(desc, BUILD_EXECUTION_KEYWORDS)
}

fn strip_build_only_acceptance_fields(acc: &mut PlanStepAcceptance) {
    acc.expect_exit_code = None;
    acc.expect_stdout_contains = None;
    acc.expect_stderr_contains = None;
    acc.expect_json_path_equals = None;
    acc.expect_http_status = None;
    // `expect_file_exists` **不再**按路径形状剔除：只读步验收「某文件已存在」是合法用法
    // （如「检查构建产物是否就绪」），而「像不像二进制」没有可靠判据。
}

/// 按 `executor_kind` 对齐验收：只读步不保留需要构建/运行证据的字段。
///
/// **只按结构化信号判定**（`executor_kind` + `is_effective`），**不读步描述文本**。
/// 原先还按步描述关键词（构建 / 只读准备 / 解压三张表）二次裁剪，已按 #1427 第 3 步移除：
/// 关键词两头不通用——词形与构建入口都枚举不完，且「分析并修复」这类描述会把**已授权改动**
/// 的步判成只读准备阶段，与 `at_turn_start` 的语义直接冲突。
pub fn align_plan_step_acceptance_with_executor_kind(step: &mut PlanStepV1) {
    if step.acceptance.as_ref().is_some_and(|a| !a.is_effective()) {
        step.acceptance = None;
    }
    if step.executor_kind == Some(PlanStepExecutorKind::ReviewReadonly)
        && let Some(acc) = step.acceptance.as_mut()
    {
        strip_build_only_acceptance_fields(acc);
    }
    if step.acceptance.as_ref().is_some_and(|a| !a.is_effective()) {
        step.acceptance = None;
    }
}

/// 对规划中每一步执行 [`normalize_plan_step_acceptance_in_place`]。
pub fn normalize_agent_reply_plan_v1_acceptance_in_place(plan: &mut AgentReplyPlanV1) {
    for step in &mut plan.steps {
        normalize_plan_step_acceptance_in_place(step);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanArtifactError {
    /// 未找到可解析且通过校验的 JSON 块
    NotFound,
    WrongType(String),
    WrongVersion(u32),
    EmptySteps,
    TooManySteps {
        max: usize,
        got: usize,
    },
    /// `no_task` 为 true 时 `steps` 必须为空。
    NoTaskWithNonEmptySteps,
    InvalidStep {
        index: usize,
        reason: &'static str,
    },
    /// `workflow_node_id` 已出现但未能覆盖 `workflow_node_ids` 中的全部节点 id（严格 PER 模式）。
    WorkflowNodesNotFullyCovered {
        missing: Vec<String>,
    },
    /// `workflow_validate_only` 后要求规划与 **`nodes[].id` 一一对应**（步数、逐步 `workflow_node_id`、多重集合一致）未满足。
    ValidateOnlyPlanNodeBindingMismatch {
        detail: &'static str,
    },
}

/// 供日志单行输出：`WrongType` 仅记长度与短预览，不记完整 `type` 字符串。
pub fn plan_artifact_error_log_summary(e: &PlanArtifactError) -> String {
    match e {
        PlanArtifactError::NotFound => "not_found".to_string(),
        PlanArtifactError::WrongType(t) => {
            let n = t.chars().count();
            let prev = crate::cm_agent::preview_chars(t, 24);
            format!("wrong_type type_len={n} type_preview={prev}")
        }
        PlanArtifactError::WrongVersion(v) => format!("wrong_version version={v}"),
        PlanArtifactError::EmptySteps => "empty_steps".to_string(),
        PlanArtifactError::TooManySteps { max, got } => {
            format!("too_many_steps max={max} got={got}")
        }
        PlanArtifactError::NoTaskWithNonEmptySteps => "no_task_with_steps".to_string(),
        PlanArtifactError::InvalidStep { index, reason } => {
            format!("invalid_step index={index} reason={reason}")
        }
        PlanArtifactError::WorkflowNodesNotFullyCovered { missing } => {
            let n = missing.len();
            let prev = missing
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(",");
            format!("workflow_nodes_not_fully_covered missing_count={n} missing_preview={prev}")
        }
        PlanArtifactError::ValidateOnlyPlanNodeBindingMismatch { detail } => {
            format!("validate_only_plan_node_binding_mismatch detail={detail}")
        }
    }
}

/// Plan v1 的完整 schema 规则长文（中文）：供维护者对照 / 文档摘录。
/// 运行时校验走类型化逻辑，**不**读取本字符串；**不要**整段注入模型「规划重写」user（见 [`PLAN_V1_REWRITE_BRIEF_RULES`]）。
pub const PLAN_V1_SCHEMA_RULES: &str = "\
- 顶层 \"type\" 为字符串 \"agent_reply_plan\"
- \"version\" 为数字 1
- 可选布尔 \"no_task\"：为 true 时表示用户未提出需分步执行的具体任务，此时 \"steps\" 必须为 []（空数组）
- 当 \"no_task\" 省略或为 false 时，\"steps\" 必须为**仅一项**的非空数组（单步规划）；但在 `workflow_validate_only` 绑定场景（每步都带 `workflow_node_id`）可出现多步；每项含非空字符串 \"id\" 与 \"description\"
- 每项 \"id\" 须唯一；**首尾不得含空白**；语法为 ASCII 字母或数字开头，仅含 - _ . /，总长不超过 128（与 workflow 节点 id 常见字符集一致）
- 可选 \"workflow_node_id\"：若填写，须**首尾无空白**且满足与 \"id\" 相同的语法；**允许在不同步骤中重复同一值**（当 `workflow_validate_only` 的 `nodes` 含重复 `id` 时，逐步绑定需要多重集一致）。值应对应最近一次 `workflow_validate_only` 工具结果里 `nodes[].id` 之一（运行时会校验子集）。在严格模式下，若**任一步**填写了 `workflow_node_id`，则**每一个**上述节点 id 都须在步骤中至少出现一次（可合并多 id 到一步时仍须逐 id 引用）
- **工作流反思 validate_only → Do**：当最近一次工具结果为 `workflow_validate_result` 且含非空 `nodes` 时，**每一步**均须设置 `workflow_node_id`，且 `steps.len()` 须**等于** `nodes` 个数；全部 `workflow_node_id` 构成的**多重集合**须与 `nodes[].id`（含重复）**完全一致**（顺序可与 DAG 不同）
- 可选 \"executor_kind\"（字符串，省略则本步不限制工具）：`review_readonly`（仅只读工具）、`patch_write`（只读 + 受限补丁写）、`test_runner`（只读 + 内置测试运行器 + `run_command`，后者仅允许配置白名单内命令）；越权调用会在工具层被拒绝并记入对话
- 可选 \"acceptance\"（对象）：确定性验收条件；**当前运行时不做硬断言**（验收校验内核已随 #1429 移除，仅保留字段与解析期对齐）；字段作为规划记录保留，勿依赖其拦截。可含：\"expect_exit_code\"（整数）、\"expect_stdout_contains\" / \"expect_stderr_contains\"（字符串子串）、\"expect_file_exists\"（工作区相对路径）、\"expect_json_path_equals\"（对象，字段 \"path\" + \"value\"；Legacy `$…` 或 JSON Pointer `/…`）、\"expect_http_status\"（整数；仅 `http_request`/`http_fetch` 类工具）
- **解压/归档步**（`archive_unpack` 等）：优先 `expect_file_exists` 指向解压后的目录或关键文件；**勿**用 `expect_stdout_contains` 填 `ls`/`run_command` 的目录列表子串（验收只读最后一条 tool 的正文）。若须匹配解压工具输出，子串须出现在 `archive_unpack` 返回文中（如「已解压」）
- 可选 \"max_step_retries\" (整数)：指定本步骤失败后允许的局部重试（打补丁）次数上限。
- 可选 \"transitions\" (数组)：状态机控制流，用于循环重试。对象含 \"condition\" (如 \"on_verify_fail\" 或 \"always\"), \"target_step_id\" (跳转目标的步骤id), \"max_loops\" (整数，最大循环次数)。触发跳转时，系统会附加一段历史记录并在界面上动态追加回退的后续步骤。
- **推荐**：有「先读后写再测」类任务时，为相应步显式设置 `executor_kind`（审阅步 `review_readonly` → 改代码步 `patch_write` → 跑测步 `test_runner`），以便每步仅暴露必要工具；合并/优化规划时**须保留**各步的 `executor_kind` 意图（可改写 `description`/`id`，勿无故清空该字段）
- **强约束（分阶段 + 验收）**：当 `executor_kind` 为 `test_runner` 且本步将跑构建/测试/静态检查时，**应**填写 `acceptance`：至少 `expect_exit_code`（多为 0），并**推荐**增加与真实 CLI 输出一致的短锚点 `expect_stdout_contains` 或 `expect_stderr_contains`，便于失败时补丁规划收到可执行反馈；纯 `review_readonly` 可省略；写后验收可配 `expect_file_exists`
- **咨询/架构类**（用户主要求分析与建议、未授权写仓库）：若仍产出可执行步，应优先 `review_readonly`；避免在单步中混用写文件意图而未设 `patch_write`（执行层会拒识越权工具）";

/// 注入模型「规划重写」user 的**短**必填说明（细则靠校验错误码回灌，勿复制 [`PLAN_V1_SCHEMA_RULES`] 全文）。
pub const PLAN_V1_REWRITE_BRIEF_RULES: &str = "\
- 顶层：`type`=\"agent_reply_plan\"，`version`=1
- 默认：`steps` 恰好 1 项，每项含非空 `id`、`description`（`id`：字母/数字开头，仅含 -_./，≤128，无首尾空白）
- 无具体任务：可设 `no_task`=true 且 `steps`=[]
- 可选：`executor_kind`（`review_readonly`|`patch_write`|`test_runner`）、`acceptance`、`workflow_node_id`、`max_step_retries`、`transitions`
- `workflow_validate_only` 绑定等多步细则：见本消息中的**校验反馈**与「补充」段（勿臆造节点 id）";

/// Plan v1 的 JSON 示例（校验/文档；重写提示优先用 [`PLAN_V1_REWRITE_EXAMPLE_JSON`]）。
pub const PLAN_V1_EXAMPLE_JSON: &str = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"verify-cargo-check","description":"在本工作区运行 cargo check 并确认通过","executor_kind":"test_runner","max_step_retries":2,"acceptance":{"expect_exit_code":0,"expect_stdout_contains":"Finished"}}]}"#;

/// 重写提示用的最短合法示例（单步，无验收细节）。
pub const PLAN_V1_REWRITE_EXAMPLE_JSON: &str = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"完成用户请求的一步并核对结果"}]}"#;

/// 将合法 v1 规划序列化为单行 JSON（供补丁助手消息写入历史）。
pub fn agent_reply_plan_v1_to_json_string(
    plan: &AgentReplyPlanV1,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(plan)
}

#[cfg(test)]
mod text_marker_tests {
    use super::*;

    /// #1422 B1 golden：英文标记走 token 边界，词内子串不再误命中。
    #[test]
    fn build_execution_ignores_word_internal_substrings() {
        assert!(!plan_step_description_implies_build_execution(
            "summarize the notes"
        ));
        // 补偿词覆盖被边界收窄掉的真实语义。
        assert!(plan_step_description_implies_build_execution(
            "rebuild the crate"
        ));
        assert!(plan_step_description_implies_build_execution(
            "building the crate"
        ));
    }

    /// 中文标记保留子串语义（中文没有词边界可依）。
    #[test]
    fn cjk_markers_keep_substring_semantics() {
        assert!(plan_step_description_implies_build_execution("编译并运行"));
    }

    /// `make ` 的尾随空格是「构建」与「Makefile 文件名」的唯一区分线索。
    #[test]
    fn trailing_space_make_marker_needs_a_following_space() {
        assert!(plan_step_description_implies_build_execution("make -j8"));
        assert!(!plan_step_description_implies_build_execution(
            "makefile cleanup"
        ));
    }

    /// #1427 第 3 步：验收裁剪只看 `executor_kind`，**不读步描述、不看路径形状**。
    ///
    /// 原实现有两条猜测通道：`expect_file_exists` 的路径形状（`/build/` 前缀、无扩展名短名），
    /// 以及步描述关键词（「查看」「分析」「解压」三张表）。两者均已移除——前者会把
    /// `src` / `docs` 当可执行产物，后者会把「分析并修复」这类**已授权改动**的步判成只读准备。
    #[test]
    fn align_uses_only_executor_kind_not_description_or_path() {
        let mut step = PlanStepV1 {
            id: "s1".into(),
            description: "查看并分析 /build/xhpcg 是否存在".into(),
            workflow_node_id: None,
            executor_kind: Some(PlanStepExecutorKind::PatchWrite),
            acceptance: Some(PlanStepAcceptance {
                expect_exit_code: Some(0),
                expect_file_exists: Some("target/debug/app".into()),
                ..PlanStepAcceptance::default()
            }),
            max_step_retries: None,
            transitions: None,
        };
        align_plan_step_acceptance_with_executor_kind(&mut step);
        let acc = step.acceptance.as_ref().expect("非只读步不得剥字段");
        assert_eq!(acc.expect_exit_code, Some(0), "步描述不参与判定");
        assert_eq!(
            acc.expect_file_exists.as_deref(),
            Some("target/debug/app"),
            "路径形状不参与判定"
        );
    }
}

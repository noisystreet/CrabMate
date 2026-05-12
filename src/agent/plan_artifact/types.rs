use std::sync::LazyLock;

use log::debug;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// 规划步骤 `id` / 可选 `workflow_node_id` 的语法：稳定、可日志引用，并与工作流节点 `id` 常见字符集对齐。
static PLAN_STEP_ID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9][-A-Za-z0-9_./]{0,127}$")
        .expect("PLAN_STEP_ID_PATTERN: 编译期正则须合法")
});

pub(crate) fn plan_step_id_syntax_ok(s: &str) -> bool {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepExecutorKind {
    /// 仅允许语义只读工具（`is_readonly_tool`）；禁止 MCP 代理工具。
    ReviewReadonly,
    /// 只读工具 + 受限写补丁类（`apply_patch` / `search_replace` / `structured_patch` / `create_file` / `modify_file` / `append_file` / `format_file` / `ast_grep_rewrite`）。
    PatchWrite,
    /// 只读工具 + 常见测试运行器（如 `cargo_test` / `pytest_run` / `go_test` 等）；**不含**任意 `run_command`。
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
    /// 可选：步骤类型，例如 `implement` 或 `verify`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_kind: Option<String>,
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

/// [`staged_plan_invalid_run_agent_turn_error`] 返回串的固定前缀；供测试、`chat_job_queue` 历史分支识别（**勿**与用户输入拼接）。当前主路径在规划 JSON 无效时已降级为常规循环，一般不再产生该串。
pub(crate) const STAGED_PLAN_INVALID_RUN_AGENT_TURN_ERROR_PREFIX: &str = "staged_plan_invalid:";

/// 供日志单行输出：`WrongType` 仅记长度与短预览，不记完整 `type` 字符串。
pub(crate) fn plan_artifact_error_log_summary(e: &PlanArtifactError) -> String {
    match e {
        PlanArtifactError::NotFound => "not_found".to_string(),
        PlanArtifactError::WrongType(t) => {
            let n = t.chars().count();
            let prev = crate::redact::preview_chars(t, 24);
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

/// 分阶段规划轮解析失败时的错误串（含结构化摘要）；主路径已改为降级，本函数供单测与兼容识别保留。
#[allow(dead_code)]
pub(crate) fn staged_plan_invalid_run_agent_turn_error(e: PlanArtifactError) -> String {
    format!(
        "{} {}",
        STAGED_PLAN_INVALID_RUN_AGENT_TURN_ERROR_PREFIX,
        plan_artifact_error_log_summary(&e)
    )
}

pub(crate) fn is_staged_plan_invalid_run_agent_turn_error(msg: &str) -> bool {
    msg.starts_with(STAGED_PLAN_INVALID_RUN_AGENT_TURN_ERROR_PREFIX)
}

/// Plan v1 的 schema 规则描述（中文），供提示词引用。
pub const PLAN_V1_SCHEMA_RULES: &str = "\
- 顶层 \"type\" 为字符串 \"agent_reply_plan\"
- \"version\" 为数字 1
- 可选布尔 \"no_task\"：为 true 时表示用户未提出需分步执行的具体任务，此时 \"steps\" 必须为 []（空数组）
- 当 \"no_task\" 省略或为 false 时，\"steps\" 必须为**仅一项**的非空数组（单步规划）；但在 `workflow_validate_only` 绑定场景（每步都带 `workflow_node_id`）可出现多步；每项含非空字符串 \"id\" 与 \"description\"
- 每项 \"id\" 须唯一；**首尾不得含空白**；语法为 ASCII 字母或数字开头，仅含 - _ . /，总长不超过 128（与 workflow 节点 id 常见字符集一致）
- 可选 \"workflow_node_id\"：若填写，须**首尾无空白**且满足与 \"id\" 相同的语法；**允许在不同步骤中重复同一值**（当 `workflow_validate_only` 的 `nodes` 含重复 `id` 时，逐步绑定需要多重集一致）。值应对应最近一次 `workflow_validate_only` 工具结果里 `nodes[].id` 之一（运行时会校验子集）。在严格模式下，若**任一步**填写了 `workflow_node_id`，则**每一个**上述节点 id 都须在步骤中至少出现一次（可合并多 id 到一步时仍须逐 id 引用）
- **工作流反思 validate_only → Do**：当最近一次工具结果为 `workflow_validate_result` 且含非空 `nodes` 时，**每一步**均须设置 `workflow_node_id`，且 `steps.len()` 须**等于** `nodes` 个数；全部 `workflow_node_id` 构成的**多重集合**须与 `nodes[].id`（含重复）**完全一致**（顺序可与 DAG 不同）
- 可选 \"executor_kind\"（字符串，省略则本步不限制工具）：`review_readonly`（仅只读工具）、`patch_write`（只读 + 受限补丁写）、`test_runner`（只读 + 内置测试运行器 + `run_command`，后者仅允许配置白名单内命令）；越权调用会在工具层被拒绝并记入对话
- 可选 \"step_kind\"（字符串）：标识步骤类型，例如 `implement` 或 `verify`（当需要进行强校验时可使用 `verify`）。
- 可选 \"acceptance\"（对象）：确定性验收条件；设定后由服务端对**本步最后一条** `role: tool` 硬断言，失败则走 **`patch_planner`** 等闭环。可含：\"expect_exit_code\"（整数）、\"expect_stdout_contains\" / \"expect_stderr_contains\"（字符串子串）、\"expect_file_exists\"（工作区相对路径）、\"expect_json_path_equals\"（对象，字段 \"path\" + \"value\"；Legacy `$…` 或 JSON Pointer `/…`）、\"expect_http_status\"（整数；仅 `http_request`/`http_fetch` 类工具）
- 可选 \"max_step_retries\" (整数)：指定本步骤失败后允许的局部重试（打补丁）次数上限。
- 可选 \"transitions\" (数组)：状态机控制流，用于循环重试。对象含 \"condition\" (如 \"on_verify_fail\" 或 \"always\"), \"target_step_id\" (跳转目标的步骤id), \"max_loops\" (整数，最大循环次数)。触发跳转时，系统会附加一段历史记录并在界面上动态追加回退的后续步骤。
- **推荐**：有「先读后写再测」类任务时，为相应步显式设置 `executor_kind`（审阅步 `review_readonly` → 改代码步 `patch_write` → 跑测步 `test_runner`），以便每步仅暴露必要工具；合并/优化规划时**须保留**各步的 `executor_kind` 意图（可改写 `description`/`id`，勿无故清空该字段）
- **强约束（分阶段 + 验收）**：当 `executor_kind` 为 `test_runner` 且本步将跑构建/测试/静态检查时，**应**填写 `acceptance`：至少 `expect_exit_code`（多为 0），并**推荐**增加与真实 CLI 输出一致的短锚点 `expect_stdout_contains` 或 `expect_stderr_contains`，便于失败时补丁规划收到可执行反馈；纯 `review_readonly` 可省略；写后验收可配 `expect_file_exists`
- **咨询/架构类**（用户主要求分析与建议、未授权写仓库）：若仍产出可执行步，应优先 `review_readonly`；避免在单步中混用写文件意图而未设 `patch_write`（执行层会拒识越权工具）";

/// Plan v1 的 JSON 示例。
pub const PLAN_V1_EXAMPLE_JSON: &str = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"verify-cargo-check","description":"在本工作区运行 cargo check 并确认通过","executor_kind":"test_runner","step_kind":"verify","max_step_retries":2,"acceptance":{"expect_exit_code":0,"expect_stdout_contains":"Finished"}}]}"#;

/// 从整段 assistant `content` 中提取并校验 v1 规划（支持 \`\`\`json / \`\`\`markdown / \`\`\`md 等带语言行的围栏，或整段即为单个 JSON 对象）。
/// 分阶段执行中：当前步工具未全部成功时，将模型返回的**补丁规划**与未完成步之后缀合并。
/// `failed_step_index` 为**零基**（对应 `plan.steps` 下标）；补丁的 `steps` 替换自该步起的后缀。
/// 将合法 v1 规划序列化为单行 JSON（供分阶段规划轮/补丁助手消息写入历史）。
pub(crate) fn agent_reply_plan_v1_to_json_string(
    plan: &AgentReplyPlanV1,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(plan)
}

/// `strict_baseline_steps`：`patch_planner` 合并结果在 `[0, failed_step_index)` 上与冻结蓝图逐步 `id` 一致。
pub(crate) fn validate_staged_patch_merged_strict_baseline_ids(
    baseline_steps: &[PlanStepV1],
    merged: &[PlanStepV1],
    failed_step_index: usize,
) -> Result<(), PlanArtifactError> {
    for i in 0..failed_step_index {
        let b = baseline_steps
            .get(i)
            .ok_or(PlanArtifactError::InvalidStep {
                index: i,
                reason: "staged_strict_baseline_prefix_missing",
            })?;
        let m = merged.get(i).ok_or(PlanArtifactError::InvalidStep {
            index: i,
            reason: "staged_strict_merged_prefix_missing",
        })?;
        if b.id.trim() != m.id.trim() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "staged_strict_baseline_step_id_mismatch",
            });
        }
    }
    Ok(())
}

pub(crate) fn merge_staged_plan_steps_after_step_failure(
    base: &[PlanStepV1],
    patch: &AgentReplyPlanV1,
    failed_step_index: usize,
) -> Result<Vec<PlanStepV1>, PlanArtifactError> {
    if patch.no_task {
        return Err(PlanArtifactError::InvalidStep {
            index: 0,
            reason: "staged_patch_no_task",
        });
    }
    if patch.steps.is_empty() {
        return Err(PlanArtifactError::EmptySteps);
    }
    if failed_step_index >= base.len() {
        return Err(PlanArtifactError::InvalidStep {
            index: failed_step_index,
            reason: "failed_step_index out of range",
        });
    }
    let mut out = Vec::with_capacity(failed_step_index + patch.steps.len());
    out.extend_from_slice(&base[..failed_step_index]);
    out.extend(patch.steps.iter().cloned());
    backfill_executor_kinds_after_staged_patch(base, &mut out, failed_step_index);
    Ok(out)
}

/// 补丁规划若省略 `executor_kind`，从**同下标**原步继承（仅覆盖被替换后缀及可能对齐的前缀位），避免 `patch_planner` 合并后子代理边界静默丢失。
fn backfill_executor_kinds_after_staged_patch(
    base: &[PlanStepV1],
    merged: &mut [PlanStepV1],
    failed_step_index: usize,
) {
    for (i, step) in merged.iter_mut().enumerate().skip(failed_step_index) {
        if step.executor_kind.is_none()
            && let Some(b) = base.get(i)
            && b.executor_kind.is_some()
        {
            step.executor_kind = b.executor_kind;
            debug!(
                target: "crabmate",
                "staged_plan_patch_backfill_executor_kind step_index={} kind={:?}",
                i,
                step.executor_kind
            );
        }
    }
}

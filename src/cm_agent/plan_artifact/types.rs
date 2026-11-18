use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

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
- **推荐**：有「先读后写再测」类任务时，为相应步显式设置 `executor_kind`（审阅步 `review_readonly` → 改代码步 `patch_write` → 跑测步 `test_runner`），以便每步仅暴露必要工具；合并/优化规划时**须保留**各步的 `executor_kind` 意图（可改写 `description`/`id`，勿无故清空该字段）
- **咨询/架构类**（用户主要求分析与建议、未授权写仓库）：若仍产出可执行步，应优先 `review_readonly`；避免在单步中混用写文件意图而未设 `patch_write`（执行层会拒识越权工具）";

/// 注入模型「规划重写」user 的**短**必填说明（细则靠校验错误码回灌，勿复制 [`PLAN_V1_SCHEMA_RULES`] 全文）。
pub const PLAN_V1_REWRITE_BRIEF_RULES: &str = "\
- 顶层：`type`=\"agent_reply_plan\"，`version`=1
- 默认：`steps` 恰好 1 项，每项含非空 `id`、`description`（`id`：字母/数字开头，仅含 -_./，≤128，无首尾空白）
- 无具体任务：可设 `no_task`=true 且 `steps`=[]
- 可选：`executor_kind`（`review_readonly`|`patch_write`|`test_runner`）、`workflow_node_id`
- `workflow_validate_only` 绑定等多步细则：见本消息中的**校验反馈**与「补充」段（勿臆造节点 id）";

/// Plan v1 的 JSON 示例（校验/文档；重写提示优先用 [`PLAN_V1_REWRITE_EXAMPLE_JSON`]）。
pub const PLAN_V1_EXAMPLE_JSON: &str = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"verify-cargo-check","description":"在本工作区运行 cargo check 并确认通过","executor_kind":"test_runner"}]}"#;

/// 重写提示用的最短合法示例（单步，无验收细节）。
pub const PLAN_V1_REWRITE_EXAMPLE_JSON: &str = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"完成用户请求的一步并核对结果"}]}"#;

/// 将合法 v1 规划序列化为单行 JSON（供补丁助手消息写入历史）。
pub fn agent_reply_plan_v1_to_json_string(
    plan: &AgentReplyPlanV1,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(plan)
}

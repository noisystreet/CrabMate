//! `dispatch_tool` 及各类需异步/阻塞池的工具执行实现。
//!
//! 进入阻塞池前对 [`AgentConfig`] 使用 [`Arc::clone`]；白名单等同理。详见仓库 `tool_registry` 模块说明。
//!
//! ## 子模块（#1198）
//!
//! 各执行阶段原先都是 `include!` 分片，文本被拼进本模块的**同一作用域**：跨阶段调用靠裸名，
//! 改一片漏改另一片编译器也无从发现（#1158 / #1198）。现在每个阶段是独立子模块，兄弟之间
//! 只能经下方**显式接口清单**互相调用——`use …::*;` 的 glob 重导出已移除。
//!
//! - `dispatch`：`dispatch_tool` 重试外壳 + 单次分发 + 各 handler 分支（编排层）。
//! - `background_job`：非 `run_command` 工具的后台任务门闩与发起。
//! - `http::{gate, tools}`：出站网络闸门（含网络轴矩阵测试）与四个出站工具入口。
//! - `terminal_session`：PTY 执行。
//! - `run_command`：`run_command` 执行流水线（#1196 已先行子模块化）。
//!
//! 行数上限（`file ≤ 920`）保持不变——本模块的目的不是把分片合并成大文件，而是把
//! 「同作用域文本拼接」换成「有边界的模块」。

use std::collections::HashSet;
use std::sync::Arc;

use crate::cm_config::AgentConfig;
use crate::cm_tool_result::ToolOutcomeFacts;
use crate::cm_types::ToolCall;

use super::meta::HandlerLookupTable;
use super::runtime::ToolRuntime;

/// 一次工具执行的**完整结果**：正文 + 结构化载荷 + 成败与错误码（#1430）。
///
/// 过去成败被拍平成 `String`，下游只能靠 `parse_tool_output` 从正文反推——`run_tool` 为此
/// 还要「按需注入 `ok` 头」再让下游解析回来（一次往返）。本类型让执行层直接给出结论。
///
/// 与线协议无关：`output` / `payload` 仍是写回 `role: tool` 与 SSE 的那两个字段，
/// 只是额外把 `ok` / `error_code` 带出来。
#[derive(Debug, Clone)]
pub struct ToolRunOutcome {
    pub output: String,
    pub payload: Option<serde_json::Value>,
    /// 执行层直接给出的成败（不再靠正文反推）。
    ///
    /// 消费方：emit 层（SSE `tool_result.ok` / `timeline` / turn replay）与
    /// `ToolOutcomeRecorder::record_tool_outcome`（统计 / 系统提示），经
    /// [`ToolOutcomeFacts`] 覆盖 [`NormalizedToolEnvelope`] 的反推结果。
    ///
    /// [`ToolOutcomeFacts`]: crate::cm_tool_result::ToolOutcomeFacts
    /// [`NormalizedToolEnvelope`]: crate::cm_tool_result::NormalizedToolEnvelope
    pub ok: bool,
    pub error_code: Option<String>,
}

impl ToolRunOutcome {
    /// 成功：正文 + 可选结构化载荷。
    pub fn ok(output: String, payload: Option<serde_json::Value>) -> Self {
        Self {
            output,
            payload,
            ok: true,
            error_code: None,
        }
    }

    /// 失败：正文 + 错误码（`None` 时下游按 `{tool}_failed` 兜底）。
    pub fn failed(output: String, error_code: Option<String>) -> Self {
        Self {
            output,
            payload: None,
            ok: false,
            error_code,
        }
    }

    /// 由正文 + 载荷构造，成败由 `parse_tool_output` 反推。
    ///
    /// **过渡用**：尚未改造的 `execute_*_impl` 仍返回 `(String, Option<Value>)`（如
    /// `run_command` / HTTP / 后台任务）。它们多半自带退出码或 `crabmate_tool_output` 头，
    /// 反推是 ①/③ 级而非 ④ 级文案兜底。等这些路径也改成返回 `ToolRunOutcome` 后可移除本函数。
    pub fn from_body_and_payload(
        tool: &str,
        output: String,
        payload: Option<serde_json::Value>,
    ) -> Self {
        let parsed = crate::cm_tool_result::parse_tool_output(tool, &output);
        Self {
            output,
            payload,
            ok: parsed.ok,
            error_code: parsed.error_code,
        }
    }

    /// 只取结构化成败事实（不搬正文）。
    ///
    /// 并行只读批按 `name+args` 缓存同一次执行的结果，回写时对每个 `tool_call_id`
    /// 各取一次事实，不需要把正文再克隆一遍。
    pub fn facts(&self) -> ToolOutcomeFacts {
        ToolOutcomeFacts::known(self.ok, self.error_code.clone())
    }

    /// 拆成 `(正文, 载荷, 成败事实)`：正文 / 载荷是写回 `role: tool` 与 SSE 的那两个字段，
    /// 事实（[`ToolOutcomeFacts`]）交给 emit / stats 定 `ok` 与 `error_code`。
    ///
    /// **纯形状转换**，不做任何判定——此前版本为诊断多做一次 `parse_tool_output`，
    /// 等于把已结构化出来的 `ok` 又用文本反推一遍，属净增开销，已移除。
    pub fn into_parts(self) -> (String, Option<serde_json::Value>, ToolOutcomeFacts) {
        let facts = self.facts();
        (self.output, self.payload, facts)
    }
}

/// Web UI：未选择工作区时的统一提示尾句（命令类工具共用）。
const WEB_WORKSPACE_PANEL_HINT: &str = "请先在右侧工作区面板设置目录（可选择目录或手动输入路径）。";

fn web_tool_err_workspace_not_set(action_zh: &str) -> String {
    format!("错误：未设置工作区，禁止{action_zh}。{WEB_WORKSPACE_PANEL_HINT}")
}

/// 在配置白名单基础上追加一条命令名（`run_command` 审批通过路径共用）。
fn extend_allowed_commands_arc(
    base: &std::sync::Arc<[String]>,
    cmd: &str,
) -> std::sync::Arc<[String]> {
    let mut v: Vec<String> = base.iter().cloned().collect();
    v.push(cmd.to_string());
    v.into()
}

pub struct DispatchToolCall<'a> {
    pub name: &'a str,
    pub args: &'a str,
    pub tc: &'a ToolCall,
}

pub struct DispatchToolWorkspace<'a> {
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub workspace_changelist:
        Option<std::sync::Arc<crate::cm_tools::workspace::changelist::WorkspaceChangelist>>,
}

impl<'a> DispatchToolWorkspace<'a> {
    /// 测试形态：工作区已设置、无变更清单。
    ///
    /// `tool_registry::tests` 的 6 个用例都要这一份「正常已设置工作区」上下文，
    /// 字段值逐字相同。生产侧三处构造（`tool_dispatch` / `exec_serial` /
    /// `parallel_readonly`）的字段来源互不相同，`&Path` 也没有合理默认值，
    /// 因此只提供测试入口而不 derive `Default`。
    #[cfg(test)]
    pub(crate) fn for_tests(effective_working_dir: &'a std::path::Path) -> Self {
        Self {
            effective_working_dir,
            workspace_is_set: true,
            workspace_changelist: None,
        }
    }
}

/// 配置、白名单与 handler 查找（每次 dispatch 必带）。
pub struct DispatchToolPolicy<'a> {
    pub cfg: &'a Arc<AgentConfig>,
    /// 多角色工具白名单；`None` 不限制。
    pub turn_allow: Option<&'a HashSet<String>>,
    /// 与 `crate::cm_internal::TurnRequest::obs` 的 `process_handles` 同源。
    pub handler_lookup: &'a HandlerLookupTable,
    pub sync_default_sandbox_backend: &'a Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend>,
    /// 本回合权限能力快照（S3 接线：命令白名单 / 路径 / 网络 / 外部工具）。
    ///
    /// `None` = 调用方尚未接线（运维 CLI、单测、内部直调），等价于现状：只按 `cfg` 判。
    /// **不要**在这里默认造一份 `project_caps`——那会让未接线路径凭空获得一个等级，
    /// 等于把「没接线」变成「按某等级静默收窄」。
    ///
    /// S3-b 起认**端口**而非具体类型：宿主可以传别的 [`crate::cm_agent::permission::PolicyEngine`]
    /// 实现；生产路径仍传 `PermissionCaps`（它实现了该 trait），行为不变。
    pub permission: Option<&'a dyn crate::cm_agent::permission::PolicyEngine>,
}

pub struct DispatchToolObs<'a> {
    pub sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    pub sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    /// 用户取消；`run_command` 等待循环会观察此标志。
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// 后台任务注册表；`None` 时 `run_command` 的 `async=true` 返回未启用（默认关闭路径）。
    pub tool_jobs: Option<std::sync::Arc<crate::jobs::tool_jobs::ToolJobRegistry>>,
}

/// 只读缓存、LTM、MCP 等附属宿主（字段不删，仅分组）。
pub struct DispatchToolMemory<'a> {
    pub read_file_turn_cache:
        Option<std::sync::Arc<crate::cm_tools::read_file_turn_cache::ReadFileTurnCache>>,
    pub long_term_memory:
        Option<Arc<crate::cm_memory::memory::long_term_memory::LongTermMemoryRuntime>>,
    pub long_term_memory_scope_id: Option<String>,
    pub mcp_turn: Option<&'a crate::mcp::McpTurnHandle>,
}

/// 单次工具分发入参（嵌套分组，避免顶层继续平铺胀袋）。
pub struct DispatchToolParams<'a> {
    pub runtime: ToolRuntime<'a>,
    pub call: DispatchToolCall<'a>,
    pub workspace: DispatchToolWorkspace<'a>,
    pub policy: DispatchToolPolicy<'a>,
    pub obs: DispatchToolObs<'a>,
    pub memory: DispatchToolMemory<'a>,
    pub delegation: DispatchToolDelegation<'a>,
}

/// 子 Agent 委托能力（#839 P1）。
///
/// **为什么是能力对象而不是凭据**：子回合要调 LLM，而 `client` / `api_key` 在工具执行层
/// **刻意不存在**（`agent_turn/host/execute/**` 中 `api_key` 零命中）。把凭据塞进本结构
/// 等于让所有工具都能读到 LLM 密钥；因此注入的是已装配好的 [`SubAgentDelegator`]，
/// 由 agent 层（持有凭据的那一侧）构造。见 `docs/design/subagent_delegation.md` §3。
///
/// `None` = 本回合未开启子 Agent 委托（`subagent_enabled = false`，默认）或调用方尚未接线。
#[derive(Clone, Copy, Default)]
pub struct DispatchToolDelegation<'a> {
    pub delegator: Option<&'a dyn crate::cm_tools::delegation_host::SubAgentDelegator>,
}

/// [`DispatchToolParams`] 中与 Docker / 配置快照相关的字段合并，降低内部分发函数的形参个数。
struct ToolExecEnv<'a> {
    cfg: &'a Arc<AgentConfig>,
    sandbox_backend: &'a Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend>,
    /// 本回合权限能力快照；`None` = 未接线（按 `cfg` 现状判定）。
    permission: Option<&'a dyn crate::cm_agent::permission::PolicyEngine>,
}

mod background_job;
mod dispatch;
mod http;
mod run_command;
mod terminal_session;

// 显式接口（#1198）：**逐项列出**兄弟子模块之间、以及 `run_command` 对外的调用面。
// 不用 glob 重导出是有意的——glob 会把「谁依赖谁」重新藏起来，正是本 Issue 要消除的问题。
// 生产侧只有这些名字需要进入 `execute` 作用域；测试专用的项由各测试文件直接 `use`
// 对应子模块（`super::dispatch::…` / `super::background_job::…`）。
pub(in crate::tool_host::tool_registry::execute) use background_job::{
    launch_background_job, try_dispatch_background_async_tool,
};
pub(in crate::tool_host::tool_registry::execute) use http::http_fetch_prefetch_approval;
pub(in crate::tool_host::tool_registry::execute) use http::{
    execute_get_weather_web, execute_http_fetch_impl, execute_http_request_impl,
    execute_web_search_web,
};
pub(in crate::tool_host::tool_registry::execute) use terminal_session::{
    TerminalSessionExecInvoke, execute_terminal_session_impl,
};

// 这两条对 `crate` 可见：`tool_registry/mod.rs` 以 `pub use execute::{…}` 对外重导出。
pub use dispatch::{dispatch_tool, prefetch_parallel_syncdefault_approvals};
pub use run_command::prefetch_http_fetch_parallel_approvals;

#[cfg(test)]
#[path = "execute_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "execute_run_command_gate_tests.rs"]
mod run_command_gate_tests;

#[cfg(test)]
#[path = "execute_allowlist_scope_tests.rs"]
mod allowlist_scope_tests;

#[cfg(test)]
#[path = "execute_read_dir_gate_tests.rs"]
mod read_dir_gate_tests;

#[cfg(test)]
#[path = "execute_background_async_tests.rs"]
mod background_async_tests;

#[cfg(test)]
#[path = "execute_run_command_allowed_roots_tests.rs"]
mod run_command_allowed_roots_tests;

#[cfg(test)]
mod tool_run_outcome_tests {
    use super::ToolRunOutcome;

    #[test]
    fn ok_constructor_carries_no_error_code() {
        let o = ToolRunOutcome::ok("done".to_string(), None);
        assert!(o.ok);
        assert!(o.error_code.is_none());
    }

    #[test]
    fn failed_constructor_marks_not_ok() {
        let o = ToolRunOutcome::failed("错误：boom".to_string(), Some("boom".to_string()));
        assert!(!o.ok);
        assert_eq!(o.error_code.as_deref(), Some("boom"));
    }

    /// `from_body_and_payload` 的成败**全凭首行前缀**判断（#1430 第 2 步的收紧口径）：
    /// 带已知前缀 ⇒ 失败；无前缀 ⇒ 成功。这条同时钉住「补前缀」与「只认前缀」两件事。
    #[test]
    fn from_body_and_payload_infers_ok_from_prefix_only() {
        let failed = ToolRunOutcome::from_body_and_payload(
            "read_file",
            "错误：读取文件失败: permission denied".to_string(),
            None,
        );
        assert!(!failed.ok, "带已知前缀的错误正文应判失败");

        let ok = ToolRunOutcome::from_body_and_payload("move_file", "已移动".to_string(), None);
        assert!(ok.ok, "无前缀的成功正文应判成功");

        // 正文里出现「失败」但**在前面**、不带前缀：按收紧口径判成功（旧宽口径会误判失败）。
        let git_log = ToolRunOutcome::from_body_and_payload(
            "run_command",
            "fix: 修复失败重试".to_string(),
            None,
        );
        assert!(git_log.ok, "正文提到失败不再误判");
    }

    /// `into_parts` 只搬形状：正文 / 载荷原样，事实与结构化成败一致。
    #[test]
    fn into_parts_is_a_shape_cast() {
        let o = ToolRunOutcome::failed("错误：x".to_string(), Some("boom".to_string()));
        let (out, payload, facts) = o.into_parts();
        assert_eq!(out, "错误：x");
        assert!(payload.is_none());
        assert_eq!(facts.ok, Some(false));
        assert_eq!(facts.error_code.as_deref(), Some("boom"));
    }
}

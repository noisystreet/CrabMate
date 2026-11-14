//! 子回合装配（#839 P0-4）：把一次「有界子任务」的输入规格翻译成 `TurnRequest`。
//!
//! 本模块**只做装配**：不实现委托工具、不注入 delegator、不改 SSE 契约。P1 的
//! `agent/delegation.rs` 只需填 `SubTurnSpec`，隔离语义在这里一次性落实，不散到各调用点。
//! 依据 `docs/design/subagent_delegation.md` §5（子回合装配）与 §12 P0-4。

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::AgentTurnLlmOverrides;
use crate::AgentTurnTransport;
use crate::RunAgentTurnAttach;
use crate::RunAgentTurnObs;
use crate::RunAgentTurnSession;
use crate::RunAgentTurnSharedInputs;
use crate::TurnRequest;
use crate::cm_agent::permission::TurnPermissionStages;
use crate::process_handles::TurnProcessHandles;
use crate::types::{LlmSeedOverride, Message, Tool};

/// 子回合的消息窗持有者。
///
/// **刻意不是裸 `Vec<Message>`**：装配器会清空传入的消息窗，若调用方顺手传了父回合的
/// `messages`，父历史就会被静默清空——不报错、不 panic，落盘时才发现对话没了。
/// 本类型字段私有、只有 [`SubTurnMessages::new`]、不实现 `From<Vec<Message>>`，
/// 于是「拿父历史来初始化」在编译期就不成立。
pub struct SubTurnMessages(Vec<Message>);

impl SubTurnMessages {
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// 只读视图：供测试断言、以及 P1 事后取回子回合的产物。
    #[must_use]
    pub fn as_slice(&self) -> &[Message] {
        &self.0
    }
}

impl Default for SubTurnMessages {
    fn default() -> Self {
        Self::new()
    }
}

/// 子回合规格：父回合的历史、工具日志与 system 全文**一概不传**，`prompt` 是唯一入口。
///
/// 字段全是「父回合已经算好的东西」的显式投影——没有一项可以被子任务自己发明，
/// 尤其是 `permission`（`min` 链结果）与 `allowed_tool_names`（只读派生后收窄）。
pub struct SubTurnSpec<'a> {
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<crate::config::AgentConfig>,
    /// 收窄后的 system 提示（**不是**父 system 的副本）。
    pub system_prompt: &'a str,
    /// 任务描述。
    pub prompt: &'a str,
    /// 继承父的工作目录与 workspace 标记：路径安全（`PathAccess` / `openat2`）依赖它们。
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    /// 只读派生的工具定义（调用方按 `registry_policy::is_readonly_tool` 过滤后的子集）。
    pub tools: &'a [Tool],
    /// `min(父等级, cfg 上限)` 的结果；本构造器**不再放宽**。
    pub permission: TurnPermissionStages,
    /// 工具白名单：只读派生后可再收窄，配置无法放大。
    pub allowed_tool_names: Option<Arc<HashSet<String>>>,
    /// 共享父的取消标志：父被取消 → 子立即收手。
    pub cancel: Option<Arc<AtomicBool>>,
    pub long_term_memory: Option<Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    pub long_term_memory_scope_id: Option<String>,
    /// 复用父的进程句柄（`read_file` 缓存与工具统计口径一致）。
    pub process_handles: Arc<TurnProcessHandles>,
    pub tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
    pub request_audit: Option<Arc<crate::WebRequestAudit>>,
    /// 单轮 `read_file` 缓存：**继承父的**（设计 §5 要求该口径一致）。
    ///
    /// 传 `None` 会让 `run_agent_turn` 按配置给子回合**另建**一份缓存，子任务读过的文件
    /// 父回合再来一遍——那正是「子任务省下的上下文」被磁盘重读抵消的地方。
    pub read_file_turn_cache: Option<Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    /// 工具审批通道（`ApprovalBroker`）。
    ///
    /// 设计 §8 要求「子回合走同一条 `dispatch_tool`，因此 `tool_approval` 自动沿用」——
    /// 那就必须真的把父的 broker 传进来。传 `None` 等于子任务失去 SSE 审批通道，
    /// 而 §8 并未就「子任务遇到需审批的工具怎么办」表态。
    ///
    /// **P1 待决策**：填父 broker，还是 `None` 并让需审批的调用直接失败？
    /// 这里显式要求调用方给值，而不是由装配器替产品决策。
    pub web_tool_ctx: Option<&'a dyn crate::cm_approval::ApprovalBroker>,
    pub llm_backend: Option<&'a (dyn crate::llm::ChatCompletionsBackend + 'static)>,
}

impl<'a> SubTurnSpec<'a> {
    /// 装配成**隔离的** `TurnRequest`；消息窗必须是 [`SubTurnMessages::new`] 建的。
    ///
    /// 隔离不变量，每条对应设计 §5 的一行；改任何一条都要同步该文档：
    ///
    /// - 消息窗先清空再写入 system + 任务描述 —— **绝不**复制父 messages（类型层面已挡住误传父历史）；
    /// - `transport.out = None` 且 `no_stream = true` —— 子回合不写父 SSE 流；
    /// - `transport.cancel` 共享父的 `Arc<AtomicBool>` —— 父取消即子收手；
    /// - `transport.per_flight` **新建** —— 子有自己的截断信号，不污染父；
    /// - `obs.tool_job_registry = None` —— 子任务不发起后台长任务；
    /// - `llm` 采样覆盖一律为默认 —— 不新开对子任务的口子。
    ///
    /// **不**在此列表的：`read_file_turn_cache` 与 `web_tool_ctx` 由调用方按 §5 / §8 填父的值，
    /// 装配器不替你决定（见 [`SubTurnSpec`] 上这两个字段的说明）。
    pub fn into_turn_request(self, window: &'a mut SubTurnMessages) -> TurnRequest<'a> {
        let messages: &'a mut Vec<Message> = &mut window.0;
        messages.clear();
        messages.push(Message::system_only(self.system_prompt));
        messages.push(Message::user_only(self.prompt));
        TurnRequest {
            shared: RunAgentTurnSharedInputs {
                client: self.client,
                api_key: self.api_key,
                cfg: self.cfg,
                tools: self.tools,
            },
            session: RunAgentTurnSession {
                messages,
                effective_working_dir: self.effective_working_dir,
                workspace_is_set: self.workspace_is_set,
            },
            transport: AgentTurnTransport {
                out: None,
                no_stream: true,
                cancel: self.cancel,
                per_flight: Some(Arc::new(crate::PerTurnFlight::default())),
                web_tool_ctx: self.web_tool_ctx,
                sse_control_mirror: None,
                llm_backend: self.llm_backend,
                trace_sink: None,
            },
            llm: AgentTurnLlmOverrides {
                temperature_override: None,
                model_override: None,
                use_executor_model: false,
                executor_model_override: None,
                executor_api_base: None,
                executor_api_key: None,
                seed_override: LlmSeedOverride::FromConfig,
            },
            attach: RunAgentTurnAttach {
                long_term_memory: self.long_term_memory,
                long_term_memory_scope_id: self.long_term_memory_scope_id,
                read_file_turn_cache: self.read_file_turn_cache,
                turn_allowed_tool_names: self.allowed_tool_names,
                permission: self.permission,
            },
            obs: RunAgentTurnObs {
                tracing_chat_turn: self.tracing_chat_turn,
                request_audit: self.request_audit,
                process_handles: self.process_handles,
                tool_job_registry: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::SessionMode;

    fn stages() -> TurnPermissionStages {
        TurnPermissionStages {
            role_max: None,
            session_mode: SessionMode::Act,
            requested: None,
            persisted: None,
        }
    }

    struct Fixture {
        client: reqwest::Client,
        cfg: Arc<crate::config::AgentConfig>,
        tools: Vec<Tool>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                client: reqwest::Client::new(),
                cfg: Arc::new(crate::cm_config::load_config(None).expect("embed default")),
                tools: Vec::new(),
            }
        }

        fn spec(&self, cancel: Option<Arc<AtomicBool>>) -> SubTurnSpec<'_> {
            SubTurnSpec {
                client: &self.client,
                api_key: "test-key",
                cfg: &self.cfg,
                system_prompt: "你是只读探索子任务",
                prompt: "看一下 x",
                effective_working_dir: std::path::Path::new("/tmp"),
                workspace_is_set: true,
                tools: &self.tools,
                permission: stages(),
                allowed_tool_names: None,
                cancel,
                long_term_memory: None,
                long_term_memory_scope_id: None,
                process_handles: crate::process_handles::TurnProcessHandles::default_arc(),
                tracing_chat_turn: None,
                request_audit: None,
                read_file_turn_cache: None,
                web_tool_ctx: None,
                llm_backend: None,
            }
        }
    }

    /// 一条测试钉住全部隔离不变量——它们是本模块存在的理由，任何一条松掉都等于
    /// 「子任务偷偷拿到了父回合的东西」。
    #[test]
    fn sub_turn_request_isolates_session_and_transport() {
        let fx = Fixture::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let dir = std::path::Path::new("/tmp");
        let mut window = SubTurnMessages::new();

        let req = fx
            .spec(Some(Arc::clone(&cancel)))
            .into_turn_request(&mut window);

        // 1) 消息窗只有 system + 任务描述
        assert_eq!(req.session.messages.len(), 2);
        // 2) 不写父 SSE 流
        assert!(req.transport.out.is_none());
        assert!(req.transport.no_stream);
        // 3) 取消标志共享父的那一份（父被取消 → 子立即收手）
        let shared_cancel = req.transport.cancel.as_ref().expect("cancel 应继承父");
        assert!(Arc::ptr_eq(shared_cancel, &cancel));
        // 4) 子有自己的截断信号，不污染父
        assert!(req.transport.per_flight.is_some());
        // 5) 子任务不发起后台长任务
        assert!(req.obs.tool_job_registry.is_none());
        // 6) 不新开采样口子
        assert!(req.llm.temperature_override.is_none());
        assert!(req.llm.model_override.is_none());
        assert!(!req.llm.use_executor_model);
        // 7) 工作目录与 workspace 标记继承（否则路径安全失效）
        assert_eq!(req.session.effective_working_dir, dir);
        assert!(req.session.workspace_is_set);
    }

    /// 复用同一个消息窗装配两次不得累积内容：装配器必须自己清干净，
    /// 不能指望调用方每次都给新的 `SubTurnMessages`。
    #[test]
    fn reused_window_does_not_accumulate() {
        let fx = Fixture::new();
        let mut window = SubTurnMessages::new();
        {
            let req = fx.spec(None).into_turn_request(&mut window);
            assert_eq!(req.session.messages.len(), 2);
        }
        let req = fx.spec(None).into_turn_request(&mut window);
        assert_eq!(
            req.session.messages.len(),
            2,
            "复用消息窗不得累积上一轮内容"
        );
    }
}

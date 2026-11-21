//! L2 跨回合任务状态（`#1386` S2 / `#1455` 的 **A 步：只落盘，不续跑**）。
//!
//! ## 为什么
//!
//! 预算耗尽时回合直接终止（`TurnBudgetDeny` → `RunAgentTurnError`），进度只留在
//! 压缩后的对话里。用户必须再发一句话，模型只能重新推断「做到哪一步」。本模块把
//! 这一刻的状态写成一份**有界、可读回**的 JSON：
//! `<working_dir>/.crabmate/tasks/<scope>.json`。
//!
//! ## 边界（A 步刻意不做，见 `docs/design/agent_autonomy_enhancement.md` §5）
//!
//! - **不含自动续跑**：没有任何生产代码读这些文件去发起新回合（C 步才做）。
//! - **不做跨回合累计预算**：`budget` 只是本回合耗尽时刻的快照（B 步才改口径）。
//! - **默认关闭**：`autonomy_enabled = false` 时调用点在进入本模块前就返回，零 IO。
//!
//! ## 三条不变量
//!
//! 1. **不存原始工具输出**：只存证据 id / 工具名 / `ok` / `exit_code`。工具 stdout 可能
//!    含凭据或用户数据，而续跑真正需要的是「哪条证据支持哪个子项」。这条也让该文件
//!    可以安全地进日志、被运维复制排查。
//! 2. **文件名只由 `scope` 白名单派生**：除 `[A-Za-z0-9_-]` 外一律替换，`.` 也不保留，
//!    因此 `../../etc/passwd` 不可能逃出 `.crabmate/tasks/`（与 `docs/AGENTS.md` §4.6 一致）。
//! 3. **写盘失败不影响回合**：调用点只 `log::warn!`；IO 错误绝不升级为回合错误。

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cm_agent::turn_budget::TurnBudgetDeny;
use crate::cm_agent::turn_origin::TurnOrigin;
use crate::cm_config::AutonomyConfig;

/// 落盘 schema 版本。
///
/// **读取只拒「未来」版本**（`> 本值`），旧版本靠 `#[serde(default)]` 补齐新增字段：
/// B 步加 `cumulative`、C 步加 `origin`，若按「不等于即拒」处理，旧文件会被整份丢弃，
/// 累计与发起者一起归零——那是数据损失，不是保守。
pub const TASK_STATE_SCHEMA: u32 = 2;

/// 工作区数据目录（与 `reminder` / `event` 同一约定）。
const DATA_DIR: &str = ".crabmate";
/// 任务状态子目录。
const TASKS_SUBDIR: &str = "tasks";
/// 文件名后缀。
const FILE_EXT: &str = "json";

/// 目标正文保留的字符上限。任务是给「续跑」看的摘要，不是存档。
pub const GOAL_MAX_CHARS: usize = 400;
/// 证据条目上限（超出保留**最新**的 N 条：续跑要先知道刚才试到哪、哪次失败了）。
pub const MAX_EVIDENCE_ITEMS: usize = 40;
/// 文件名字符上限（`scope` 截断阈值）。
pub const SCOPE_MAX_CHARS: usize = 64;

/// 任务状态。A 步只会写出 [`Self::Paused`]；`Done` 留给 B / C 步。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// 因预算耗尽等原因中止，进度已保留。
    Paused,
}

impl TaskStatus {
    /// 稳定标识（日志 / 金样 / 将来的 HTTP 呈现）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paused => "paused",
        }
    }
}

/// 暂停原因。**与 [`TurnBudgetDeny`] 一一对应**，不引入自由文本：
/// 自由文本会被下游按文案匹配，改一句提示就静默改语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPauseReason {
    /// 触到单轮墙钟上限。
    BudgetWallClock,
    /// 触到单轮 LLM 调用次数上限。
    BudgetLlmCalls,
    /// 触到单轮 Token 粗估上限。
    BudgetTokens,
    /// `#1455` C 步：卡在工具审批上，等人决定（不是预算问题）。
    ///
    /// 与三个 `Budget*` 的区别决定续跑行为：预算暂停**可以**自动续跑，
    /// 这条**不可以**（`autonomy_stop_on_approval_required` 默认 true）——等人是安全边界。
    AwaitingApproval,
}

impl TaskPauseReason {
    /// 稳定标识。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BudgetWallClock => "budget_wall_clock",
            Self::BudgetLlmCalls => "budget_llm_calls",
            Self::BudgetTokens => "budget_tokens",
            Self::AwaitingApproval => "awaiting_approval",
        }
    }
}

impl From<TurnBudgetDeny> for TaskPauseReason {
    fn from(d: TurnBudgetDeny) -> Self {
        match d {
            TurnBudgetDeny::WallClock => Self::BudgetWallClock,
            TurnBudgetDeny::LlmCalls => Self::BudgetLlmCalls,
            TurnBudgetDeny::Tokens => Self::BudgetTokens,
        }
    }
}

/// 一条证据的**指针**（不含输出正文，见模块文档不变量 1）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEvidenceRef {
    /// 与第 ① 层证据层同一套 id（`ev_1`…），供续跑时回引。
    pub id: String,
    /// `verify` / `write` / `other`（沿用 `EvidenceKind::as_str`）。
    pub kind: String,
    /// 产生该证据的工具名。
    pub tool: String,
    /// 该次调用是否成功。
    pub ok: bool,
    /// 结构化退出码（专用工具才有）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// 暂停时刻的本回合预算快照（**单轮**口径）。
///
/// B 步起 [`TaskState::cumulative`] 复用同一形状表示**跨回合累计**——两个口径分开存，
/// 是因为「本回合用了多少」与「一共用了多少」在续跑判定里含义不同，混在一处必生歧义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaskBudgetSnapshot {
    pub llm_calls: u32,
    pub outer_loop_iterations: u32,
    pub estimated_tokens: usize,
    pub elapsed_secs: u64,
}

/// 一份任务状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskState {
    /// 落盘 schema（读取方据此拒绝未知形状，不做猜测式迁移）。
    pub schema: u32,
    /// 派生文件名的会话标识（`conversation_id` 或 CLI 的固定 `cli`）。
    pub scope_id: String,
    /// 目标摘要（已按 [`GOAL_MAX_CHARS`] 截断）。
    pub goal: String,
    pub status: TaskStatus,
    pub pause_reason: TaskPauseReason,
    pub budget: TaskBudgetSnapshot,
    /// **累计到暂停时刻**的三项消耗（B 步新增；不含暂停等待时间）。
    ///
    /// 旧文件（A 步产物）没有该字段 ⇒ serde 落到全 `0`，即「未知即 0」——
    /// 与「读失败当 0」的 fail-open 口径一致，不引入迁移分支。
    #[serde(default)]
    pub cumulative: TaskBudgetSnapshot,
    /// 本轮是怎么发起的（`#1455` C 步）。与「发了什么」正交；旧文件缺省为 `User`。
    #[serde(default)]
    pub origin: TurnOrigin,
    pub evidence: Vec<TaskEvidenceRef>,
    /// 最后写入时刻（RFC3339，UTC）。
    pub updated_at: String,
}

impl TaskState {
    /// 构造一份 `paused` 状态，并就地施加全部上界（目标截断、证据裁剪）。
    ///
    /// 截断与裁剪放在构造器而不是调用点：让「写出去的永远不会超界」成为类型级承诺，
    /// 调用点无法忘记。
    #[must_use]
    pub fn new_paused(
        scope_id: &str,
        goal: &str,
        reason: TaskPauseReason,
        budget: TaskBudgetSnapshot,
        evidence: Vec<TaskEvidenceRef>,
    ) -> Self {
        Self {
            schema: TASK_STATE_SCHEMA,
            scope_id: sanitize_scope(scope_id),
            goal: truncate_chars(goal.trim(), GOAL_MAX_CHARS),
            status: TaskStatus::Paused,
            pause_reason: reason,
            budget,
            // 首次暂停时「累计」自然等于「本次」——没有更早的历史。
            cumulative: budget,
            origin: TurnOrigin::User,
            evidence: cap_evidence(evidence),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// 用真实累计值覆盖 [`Self::cumulative`]（B 步接线用；构造器保持单参不改调用点）。
    #[must_use]
    pub fn with_cumulative(mut self, cumulative: TaskBudgetSnapshot) -> Self {
        self.cumulative = cumulative;
        self
    }
}

/// 自动续跑的决定（`#1455` C 步）。纯函数产出，便于把八种组合各自钉住。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoResumeDecision {
    /// 允许自动续跑。
    Go,
    /// 不续跑，带上原因（只用于日志与排障，不面向用户）。
    Stop(AutoResumeStopReason),
}

/// 不自动续跑的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoResumeStopReason {
    /// `enabled` 或 `auto_resume` 关闭（默认）。
    Disabled,
    /// 本回合**不是**因预算暂停结束（正常完成或其它失败）。
    NotPaused,
    /// 暂停在工具审批上，且配置为「遇审批即停」（默认）。
    AwaitingApproval,
    /// 已达单会话自动续跑轮数上限（**硬闸**）。
    MaxTurnsReached,
    /// 连续因**同一原因**暂停，判定为「续跑没有换到新状态」——提前止损。
    ///
    /// 与 [`Self::MaxTurnsReached`] 的区别：轮数是绝对上限，本项是**效率止损**
    /// （N 轮可能几秒内就烧完且毫无进展）。
    NoProgress,
    /// 工作区或权限等级在本回合前后变了——不续跑，避免绕过人工切换。
    ContextChanged,
}

impl AutoResumeStopReason {
    /// 稳定标识（日志 / 排障）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::NotPaused => "not_paused",
            Self::AwaitingApproval => "awaiting_approval",
            Self::MaxTurnsReached => "max_turns_reached",
            Self::NoProgress => "no_progress",
            Self::ContextChanged => "context_changed",
        }
    }
}

/// 判定「本回合结束后是否自动再起一个回合」。
///
/// 顺序是**安全优先**：先看开关，再看是否真因预算暂停，再看审批边界，然后是轮数硬闸、
/// 无进展止损，最后是上下文变化。原因在于：让「开关关闭」永远先于其它判断返回，才能保证默认配置下
/// 这个函数是纯读、零副作用（不续跑、不计数）。
///
/// **硬闸**（`max_autonomous_turns`）的计数由调用方持有且必须是**进程内**的——
/// 不能读状态文件，否则删文件即可绕过。
#[must_use]
pub fn decide_auto_resume(
    cfg: &AutonomyConfig,
    paused_reason: Option<TaskPauseReason>,
    resumed_turns_so_far: u32,
    consecutive_same_reason: u32,
    context_changed: bool,
) -> AutoResumeDecision {
    if !cfg.enabled || !cfg.auto_resume {
        return AutoResumeDecision::Stop(AutoResumeStopReason::Disabled);
    }
    let Some(reason) = paused_reason else {
        return AutoResumeDecision::Stop(AutoResumeStopReason::NotPaused);
    };
    if reason == TaskPauseReason::AwaitingApproval && cfg.stop_on_approval_required {
        return AutoResumeDecision::Stop(AutoResumeStopReason::AwaitingApproval);
    }
    if cfg.max_autonomous_turns > 0 && resumed_turns_so_far >= cfg.max_autonomous_turns {
        return AutoResumeDecision::Stop(AutoResumeStopReason::MaxTurnsReached);
    }
    // 无进展止损：约束的是「续跑是否有效」，不是「还能不能续」。放在轮数硬闸之后——
    // 轮数是绝对上限，无进展只是提前停止，不该抢在硬闸前面。
    if consecutive_same_reason >= crate::cm_agent::run_epoch::MAX_CONSECUTIVE_SAME_REASON {
        return AutoResumeDecision::Stop(AutoResumeStopReason::NoProgress);
    }
    if context_changed {
        return AutoResumeDecision::Stop(AutoResumeStopReason::ContextChanged);
    }
    AutoResumeDecision::Go
}

/// 注入给模型的**精简**任务状态摘要字符上限。
///
/// 只作对照：既有 `session_workspace_changelist` 的注入比这大一量级，本摘要刻意更小——
/// 它的作用是「别从头重做」，不是复述历史。
pub const TASK_BRIEF_MAX_CHARS: usize = 800;
/// 摘要里列出的最近证据条数（取**最新**，与证据裁剪方向一致）。
pub const TASK_BRIEF_EVIDENCE_ITEMS: usize = 3;

/// 渲染注入模型的任务状态摘要。
///
/// 只含目标、暂停原因、累计预算与最近证据**指针**——不含任何工具原始输出
/// （与模块不变量 1 同源，因此从 [`TaskState`] 渲染天然不泄露凭据/用户数据）。
#[must_use]
pub fn render_task_brief(state: &TaskState) -> String {
    let mut out = String::new();
    out.push_str("[任务上下文] 本会话此前因预算中断，进度如下。请据此继续，不要从头重做。\n");
    out.push_str(&format!("目标：{}\n", state.goal));
    out.push_str(&format!("暂停原因：{}\n", state.pause_reason.as_str()));
    out.push_str(&format!(
        "累计已用：LLM 调用 {} 次 / 约 {} tokens / 墙钟 {} 秒\n",
        state.cumulative.llm_calls,
        state.cumulative.estimated_tokens,
        state.cumulative.elapsed_secs
    ));
    if !state.evidence.is_empty() {
        let tail: Vec<String> = state
            .evidence
            .iter()
            .rev()
            .take(TASK_BRIEF_EVIDENCE_ITEMS)
            .map(|e| {
                let code = e
                    .exit_code
                    .map(|c| format!(" exit_code={c}"))
                    .unwrap_or_default();
                format!("{} {} ok={} tool={}{}", e.id, e.kind, e.ok, e.tool, code)
            })
            .collect();
        out.push_str(&format!("最近证据：{}\n", tail.join("；")));
    }
    truncate_chars(&out, TASK_BRIEF_MAX_CHARS)
}

/// 供 `prepare_messages_for_model`：先移除旧注入条，再视需要插入最新摘要。
///
/// 与 `sync_changelist_user_message` 同一模式：**运行时每次刷新**，
/// 落盘前由 `server_injected_user::strip_runtime_refreshed_injections` 整体剥离。
pub fn sync_paused_task_brief_user_message(
    messages: &mut Vec<crate::cm_types::Message>,
    brief: Option<&str>,
) {
    messages.retain(|m| !crate::cm_types::is_paused_task_injection(m));
    let Some(text) = brief.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if messages.is_empty() {
        return;
    }
    messages.push(crate::cm_types::Message {
        role: "user".to_string(),
        content: Some(text.to_string().into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: Some(crate::cm_types::CRABMATE_PAUSED_TASK_NAME.to_string()),
        tool_call_id: None,
    });
}

/// 按 Unicode scalar 截断，超出时补省略号（不切半个 UTF-8）。
#[must_use]
pub fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// 把任意 scope 归一为安全文件名主干。
///
/// 白名单而非黑名单：`/` `\` `.` `:` 与不可打印字符全部落成 `_`，因此既不可能出现
/// `..` 段，也不可能出现绝对路径。空结果回退 `unknown`（否则会得到 `.json` 这种隐藏文件）。
#[must_use]
pub fn sanitize_scope(scope: &str) -> String {
    let mut out = String::new();
    for ch in scope.chars().take(SCOPE_MAX_CHARS) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

/// 证据裁剪：保留**最新**的 N 条。
///
/// 方向不能反：续跑最需要知道「刚才试到哪、哪次失败了」，而不是「最早读了哪些文件」。
/// 首版按「保留最早」实现，会把长回合里最关键的尾部失败证据丢掉。
fn cap_evidence(mut evidence: Vec<TaskEvidenceRef>) -> Vec<TaskEvidenceRef> {
    if evidence.len() > MAX_EVIDENCE_ITEMS {
        let drop_count = evidence.len() - MAX_EVIDENCE_ITEMS;
        evidence.drain(..drop_count);
    }
    evidence
}

/// `.crabmate/tasks/` 的读写与清理。
pub struct TaskStateStore<'a> {
    root: &'a Path,
    max_tasks: usize,
    ttl_secs: u64,
}

impl<'a> TaskStateStore<'a> {
    /// 以工作区根与 `[agent.autonomy]` 配置构造。
    #[must_use]
    pub fn new(root: &'a Path, cfg: &AutonomyConfig) -> Self {
        Self {
            root,
            max_tasks: cfg.max_tasks,
            ttl_secs: cfg.task_ttl_secs,
        }
    }

    /// 任务状态目录（`<root>/.crabmate/tasks`）。
    #[must_use]
    pub fn tasks_dir(&self) -> PathBuf {
        self.root.join(DATA_DIR).join(TASKS_SUBDIR)
    }

    /// 某个 scope 对应的文件路径。
    #[must_use]
    pub fn path_for(&self, scope_id: &str) -> PathBuf {
        self.tasks_dir()
            .join(format!("{}.{FILE_EXT}", sanitize_scope(scope_id)))
    }

    /// 写一份状态并顺带清理旧文件。
    ///
    /// **同一 scope 覆盖同一文件**：`conversation_id` 是天然主键，所以长期运行不会
    /// 每回合新增一个文件（这是最省事的「不堆积」保证）。
    pub fn save(&self, state: &TaskState) -> io::Result<PathBuf> {
        let dir = self.tasks_dir();
        std::fs::create_dir_all(&dir)?;
        let path = self.path_for(&state.scope_id);
        let mut bytes = serde_json::to_vec_pretty(state)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        bytes.push(b'\n');
        write_atomic(&path, &bytes)?;
        // 清理失败不影响主写入（旧文件只是占空间，不影响本次落盘的正确性）。
        let _ = self.prune();
        Ok(path)
    }

    /// 读回某个 scope 的状态。文件不存在 → `Ok(None)`；schema 不认识 → `Ok(None)`
    /// （旧/新版本文件不是错误，而是「不可用」）。
    pub fn load(&self, scope_id: &str) -> io::Result<Option<TaskState>> {
        let path = self.path_for(scope_id);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let parsed: TaskState = match serde_json::from_slice(&bytes) {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };
        // 只拒**未来**版本（见 `TASK_STATE_SCHEMA` 的说明）：旧文件的缺失字段由
        // `#[serde(default)]` 补齐，不该因版本号落后而整份不可用。
        if parsed.schema > TASK_STATE_SCHEMA {
            return Ok(None);
        }
        Ok(Some(parsed))
    }

    /// 删除超期与超量的文件，返回删除个数。
    ///
    /// - `ttl_secs == 0`：不按时间清理；
    /// - `max_tasks == 0`：不按数量清理。
    ///
    /// 先按 TTL 删，再按修改时间从旧到新删到上限以内——顺序不能反：先按数量删会把
    /// 「本来就该过期」的文件留在上限之内。
    pub fn prune(&self) -> io::Result<usize> {
        if self.ttl_secs == 0 && self.max_tasks == 0 {
            return Ok(0);
        }
        let entries = match std::fs::read_dir(self.tasks_dir()) {
            Ok(e) => e,
            // 目录还不存在 = 没有任何状态文件，不是错误。
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let (expired, kept) = self.drop_expired(entries);
        Ok(expired + self.trim_to_max_tasks(kept))
    }

    /// 删掉超期文件，返回（删除个数, 存活文件及其修改时间）。
    fn drop_expired(
        &self,
        entries: std::fs::ReadDir,
    ) -> (usize, Vec<(std::time::SystemTime, PathBuf)>) {
        let now = std::time::SystemTime::now();
        let mut kept: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        let mut removed = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some(FILE_EXT) {
                continue;
            }
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            if self.ttl_secs > 0 && is_expired(now, mtime, self.ttl_secs) {
                // 删不掉（权限 / 已被别的回合删了）不计入，也不报错。
                if std::fs::remove_file(&path).is_ok() {
                    removed += 1;
                }
                continue;
            }
            kept.push((mtime, path));
        }
        (removed, kept)
    }

    /// 按修改时间从旧到新删到上限以内，返回删除个数。
    fn trim_to_max_tasks(&self, mut kept: Vec<(std::time::SystemTime, PathBuf)>) -> usize {
        if self.max_tasks == 0 || kept.len() <= self.max_tasks {
            return 0;
        }
        kept.sort_by_key(|a| a.0);
        let excess = kept.len() - self.max_tasks;
        let mut removed = 0usize;
        for (_, path) in kept.into_iter().take(excess) {
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

/// `now - mtime > ttl`；时钟回拨（mtime 在未来）一律**不**判过期。
fn is_expired(
    now: std::time::SystemTime,
    mtime: std::time::SystemTime,
    ttl_secs: u64,
) -> bool {
    match now.duration_since(mtime) {
        Ok(age) => age.as_secs() > ttl_secs,
        Err(_) => false,
    }
}

/// 原子写：先写同目录临时文件再 `rename`，避免读到半个 JSON。
///
/// 并发：同一 scope 的两个回合可能同时写。`rename` 是原子的，结果必然是其中一份
/// 完整文件（后者胜），不会出现交错内容——这比加锁更简单，且这里不需要顺序保序。
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // 临时文件名**必须唯一**：首版用固定的 `<name>.json.tmp`，`cm_ctl` 支持的多实例共享同一工作区时，
    // 两个进程写同一 scope 会落到同一 tmp 路径——A 的 `rename` 把 inode 搬走之后，B 仍持有该 inode 的
    // 写句柄，于是**直接截断目标文件**，读到的可能是半截 JSON。pid + 进程内序号即可两者都覆盖。
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{FILE_EXT}.{}.{seq}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
#[path = "task_state_tests.rs"]
mod tests;

//! 权限决策的结构化审计事件（设计 `docs/design/permission_levels.md` §11.2）。
//!
//! L4（`unrestricted`）不弹审批，它的价值完全建立在「放行留痕」上：没有留痕的
//! `AuditOnly` 就等于静默提权。本模块只做一件事——把执行面的放行 / 拒绝 / 人工审批结论
//! 收成一条结构化事件。
//!
//! **为什么在 `cm_types` 而不是 `cm_agent`**：路径轴要在 `cm_tools::tools::file::PathAccess`
//! （`cm_tools` 层）记录，而依赖方向是 `cm_types` → `cm_config` → `cm_tools` → `cm_agent`——
//! 放在 `cm_agent` 会让叶子层反向依赖。本模块只依赖 [`PermissionLevel`] 与 [`ApprovalPolicy`]
//! （两者都已在 `cm_types`），无 IO、无配置。上层便捷包装见 `crate::cm_agent::permission_audit`。
//!
//! **为什么先用 tracing 而不是 SSE 审计事件**：§11.3 的 control-plane 方案需要双端同步
//! （`cm_sse_protocol` + Client 解析器 + 协议 golden），留给 Client 那一波；tracing 零协议
//! 变更、可在本仓独立合并。
//!
//! **feature 门控**：`tracing` 在 Cargo.toml 里是 `server` 的可选依赖，而本模块挂在始终编译的
//! `cm_types` 下（`protocol` 构建也要过）。所以 `emit` 只在实际要发事件的 `server` 构建里
//! 走 tracing；`protocol`（Client WASM）构建退化为空实现，纯类型与 `should_emit` 判定照旧可用。
//!
//! **字段名为什么是下划线**：tracing 的字段名必须是 Rust 标识符，§11.2 写的
//! `permission.level` 只能表达为 `permission_level`；检索统一用 `target: "permission_audit"`。

use super::permission_level::{ApprovalPolicy, PermissionLevel};

/// 审计轴（§11.2）：越界发生在哪一轴上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionAxis {
    /// 工具可见性 / early-deny 的通用否决，以及 workflow 节点级放行。
    Tool,
    /// 文件路径（工作区外、`..`）。
    Path,
    /// 命令白名单与 shell 包装。
    Command,
    /// 出站网络前缀。
    Net,
    /// 外部工具（`mcp__*` / `plugins/*.json` 动态工具）。
    External,
}

impl PermissionAxis {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PermissionAxis::Tool => "tool",
            PermissionAxis::Path => "path",
            PermissionAxis::Command => "command",
            PermissionAxis::Net => "net",
            PermissionAxis::External => "external",
        }
    }
}

/// 审计结论（§11.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// 按等级**直接放行**（不弹审批）。只有 `AuditOnly`（L4）会记这一种。
    Allowed,
    /// 按等级**直接拒绝**（不弹审批）。
    Denied,
    /// 人工审批（`Gate`）给出了结论。
    Approved,
}

impl PermissionDecision {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PermissionDecision::Allowed => "allowed",
            PermissionDecision::Denied => "denied",
            PermissionDecision::Approved => "approved",
        }
    }
}

/// 一条审计记录（与 tracing 输出同源，故可脱离 subscriber 单测）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionAuditEvent<'a> {
    pub level: PermissionLevel,
    pub axis: PermissionAxis,
    pub decision: PermissionDecision,
    /// 相关工具名（命令 / 网络 / 路径轴也带工具名，便于按工具聚合）。
    pub tool: Option<&'a str>,
    /// 越界对象的一句话摘要（命令名、origin、路径末段）。**不得**含密钥与完整 URL query。
    pub detail: Option<&'a str>,
}

/// 该结论是否值得记一条事件。
///
/// 判定规则是本切片的语义核心：
///
/// - `Denied` / `Approved` 一律记——它们是「边界真的动了」的稀有事件；
/// - `Allowed` **仅** `AuditOnly`（L4）记：L0–L3 下每个白名单命中的命令都会触发一次，
///   记进来只会把「L4 到底放行了什么」这批真正需要审计的记录淹没。
#[must_use]
pub const fn should_emit(approval: ApprovalPolicy, decision: PermissionDecision) -> bool {
    match decision {
        PermissionDecision::Denied | PermissionDecision::Approved => true,
        PermissionDecision::Allowed => matches!(approval, ApprovalPolicy::AuditOnly),
    }
}

/// 发一条审计事件。`should_emit` 的判定由调用方先做（见 [`record`] 等）。
#[cfg(feature = "server")]
pub fn emit(event: &PermissionAuditEvent<'_>) {
    let level = event.level.as_str();
    let axis = event.axis.as_str();
    let decision = event.decision.as_str();
    let tool = event.tool.unwrap_or("-");
    let detail = event.detail.unwrap_or("-");
    // 拒绝走 warn（运维要能直接 grep 出「被谁拦了」），其余走 info。
    if event.decision == PermissionDecision::Denied {
        tracing::warn!(
            target: "permission_audit",
            permission_level = level,
            permission_axis = axis,
            permission_decision = decision,
            tool = tool,
            detail = detail,
            "permission denied by level"
        );
    } else {
        tracing::info!(
            target: "permission_audit",
            permission_level = level,
            permission_axis = axis,
            permission_decision = decision,
            tool = tool,
            detail = detail,
            "permission decision"
        );
    }
}

/// `protocol`（Client WASM）构建没有 `tracing` 依赖：审计是服务端行为，这里退化为空实现，
/// 好让 [`record`] 等调用面在两种 feature 组合下签名一致（见模块文档「feature 门控」）。
#[cfg(not(feature = "server"))]
pub fn emit(_event: &PermissionAuditEvent<'_>) {}

/// 记一条事件（内部按 [`should_emit`] 过滤）。
pub fn record(
    level: PermissionLevel,
    approval: ApprovalPolicy,
    axis: PermissionAxis,
    decision: PermissionDecision,
    tool: Option<&str>,
    detail: Option<&str>,
) {
    if !should_emit(approval, decision) {
        return;
    }
    emit(&PermissionAuditEvent {
        level,
        axis,
        decision,
        tool,
        detail,
    });
}

/// 等级**直接放行**（L4 的核心留痕）。L0–L3 下是空操作（见 [`should_emit`]）。
pub fn record_allowed(
    level: PermissionLevel,
    approval: ApprovalPolicy,
    axis: PermissionAxis,
    tool: Option<&str>,
    detail: Option<&str>,
) {
    record(
        level,
        approval,
        axis,
        PermissionDecision::Allowed,
        tool,
        detail,
    );
}

/// 等级**直接拒绝**（L0–L2 的硬边界，L4 的 external 轴也可能拒）。
pub fn record_denied(
    level: PermissionLevel,
    approval: ApprovalPolicy,
    axis: PermissionAxis,
    tool: Option<&str>,
    detail: Option<&str>,
) {
    record(
        level,
        approval,
        axis,
        PermissionDecision::Denied,
        tool,
        detail,
    );
}

/// 人工审批（`Gate`）给出了**批准**结论：`AllowOnce` / `AllowAlways` 都要留痕。
///
/// 与 `Allowed` 的区别：那是等级本身就不设限，这是「人点了确认」。二者都意味着
/// 「这一步越过了默认边界」，所以都要记——但结论串不同，便于事后区分责任来源。
pub fn record_approved(
    level: PermissionLevel,
    axis: PermissionAxis,
    tool: Option<&str>,
    detail: Option<&str>,
) {
    // `Approved` 与 approval 策略无关（`should_emit` 恒真），故不需要 `approval` 入参。
    emit(&PermissionAuditEvent {
        level,
        axis,
        decision: PermissionDecision::Approved,
        tool,
        detail,
    });
}

// 捕获 subscriber 依赖 `tracing`，故测试也只随 `server` feature 编译（`protocol` 构建没有该依赖）。
#[cfg(all(test, feature = "server"))]
#[path = "permission_audit_tests.rs"]
mod tests;

/// 测试侧共享：在局部 subscriber 下捕获 `permission_audit` 事件正文。
///
/// 执行点的接线测试（`http_net_gate`、`PathAccess` 等）都要「证明确实发了一条」，
/// 判定函数（[`should_emit`]）本身证明不了这一点。
#[cfg(all(test, feature = "server"))]
pub(crate) mod test_support {
    use std::sync::{Arc, Mutex};

    /// 捕获 `tracing` 事件正文的最小 subscriber。
    struct CaptureSubscriber {
        lines: Arc<Mutex<Vec<String>>>,
    }

    #[derive(Default)]
    struct FieldVisitor {
        out: String,
    }

    impl tracing::field::Visit for FieldVisitor {
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.out.push_str(&format!("{}={value} ", field.name()));
        }

        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.out.push_str(&format!("{}={value:?} ", field.name()));
        }
    }

    impl tracing::Subscriber for CaptureSubscriber {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let mut visitor = FieldVisitor::default();
            event.record(&mut visitor);
            let line = format!("{} {}", event.metadata().target(), visitor.out);
            if let Ok(mut lines) = self.lines.lock() {
                lines.push(line);
            }
        }
    }

    /// 执行 `f`，返回期间捕获的事件正文（每行一条，`target + 字段`）。
    pub(crate) fn captured_with(f: impl FnOnce()) -> String {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = CaptureSubscriber {
            lines: Arc::clone(&lines),
        };
        tracing::subscriber::with_default(sink, f);
        lines.lock().map(|l| l.join("\n")).unwrap_or_default()
    }
}

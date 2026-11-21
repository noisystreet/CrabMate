//! 一轮对话的**发起者**（`#1455` C 步）。
//!
//! 与「这一轮发了什么内容」**正交**：内容由消息表达，来源由本类型表达。
//! 之前把两者压进 `message.name`，导致每加一类触发者都要同时改三处判定
//! （`is_server_injected_user_message` 白名单、`is_runtime_refreshed_injection` 归类、
//! 落盘 strip 保留名单）。本类型把「谁发起」抽出来，触发者接入时只加一个变体 + 一条决策分支。
//!
//! 放 `cm_agent` 而不是 `agent`：`TaskState`（本模块的邻居）要记录它，
//! 而 `agent` 依赖 `cm_agent`，反向引用会破坏依赖方向。

use serde::{Deserialize, Serialize};

/// 一轮对话的发起者。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOrigin {
    /// 真实用户经 `POST /chat`、`/chat/stream` 或 CLI 发起（默认）。
    #[default]
    User,
    /// 服务端在预算暂停后**自动**续跑（`#1455` C 步）。
    AutoResume,
    /// `[[scheduled_agent_task]]` 的 cron 触发。
    Scheduled,
    /// 父回合派生的**隔离**子回合（`delegate_task`）。
    SubTurn,
    /// 外部 webhook / 集成触发。**预留**：当前无生产者，接入时再补决策分支。
    Webhook,
}

impl TurnOrigin {
    /// 稳定标识（日志 / 落盘 / 将来的 HTTP 呈现）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::AutoResume => "auto_resume",
            Self::Scheduled => "scheduled",
            Self::SubTurn => "sub_turn",
            Self::Webhook => "webhook",
        }
    }

    /// 映射到既有 [`crate::request_audit::WebRequestAudit`] 的 `source` 取值。
    ///
    /// 审计面的 `source` 是**更粗**的一组（`http` / `scheduled`）：它回答「HTTP 还是定时」，
    /// 而本类型回答「谁发起」。两者不是一一对应，故显式映射而不是复用同一个字符串。
    #[must_use]
    pub const fn audit_source(self) -> &'static str {
        match self {
            Self::User | Self::AutoResume | Self::SubTurn => "http",
            Self::Scheduled => "scheduled",
            Self::Webhook => "webhook",
        }
    }

    /// 是否为「服务端代用户发起」的一类（续跑 / 定时 / 外部触发）。
    ///
    /// 用于问「这一轮是否代表用户意愿」——`SubTurn` 虽非用户直发，但它服务于父回合，
    /// 语义上仍属用户授权的任务，故**不**计入此类。
    #[must_use]
    pub const fn is_server_initiated(self) -> bool {
        matches!(self, Self::AutoResume | Self::Scheduled | Self::Webhook)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_user() {
        assert_eq!(TurnOrigin::default(), TurnOrigin::User);
    }

    /// 落盘 / 日志用的标识必须稳定：改名等于改契约。
    #[test]
    fn stable_strings() {
        assert_eq!(TurnOrigin::User.as_str(), "user");
        assert_eq!(TurnOrigin::AutoResume.as_str(), "auto_resume");
        assert_eq!(TurnOrigin::Scheduled.as_str(), "scheduled");
        assert_eq!(TurnOrigin::SubTurn.as_str(), "sub_turn");
        assert_eq!(TurnOrigin::Webhook.as_str(), "webhook");
    }

    #[test]
    fn audit_source_is_coarser_than_origin() {
        // 三种「经由 HTTP 进来」的来源共用一个审计取值。
        for o in [TurnOrigin::User, TurnOrigin::AutoResume, TurnOrigin::SubTurn] {
            assert_eq!(o.audit_source(), "http", "{o:?}");
        }
        assert_eq!(TurnOrigin::Scheduled.audit_source(), "scheduled");
        assert_eq!(TurnOrigin::Webhook.audit_source(), "webhook");
    }

    /// `SubTurn` 服务父回合，不算「代用户发起」。
    #[test]
    fn server_initiated_excludes_sub_turn() {
        assert!(TurnOrigin::AutoResume.is_server_initiated());
        assert!(TurnOrigin::Scheduled.is_server_initiated());
        assert!(TurnOrigin::Webhook.is_server_initiated());
        assert!(!TurnOrigin::User.is_server_initiated());
        assert!(!TurnOrigin::SubTurn.is_server_initiated());
    }

    #[test]
    fn serde_roundtrip_uses_snake_case() {
        for o in [
            TurnOrigin::User,
            TurnOrigin::AutoResume,
            TurnOrigin::Scheduled,
            TurnOrigin::SubTurn,
            TurnOrigin::Webhook,
        ] {
            let json = serde_json::to_string(&o).expect("serialize");
            assert_eq!(json, format!("\"{}\"", o.as_str()));
            let back: TurnOrigin = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, o);
        }
    }
}

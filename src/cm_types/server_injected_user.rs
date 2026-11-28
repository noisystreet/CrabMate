//! 服务端注入的 **`role: user`** 消息：注册表、识别与落盘剥离（非用户真实发言）。

use crate::cm_types::message::{
    CRABMATE_AUTO_RESUME_NAME, CRABMATE_CONTEXT_SUMMARY_NAME,
    CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, CRABMATE_FORCED_SKILL_NAME,
    CRABMATE_LONG_TERM_MEMORY_NAME, CRABMATE_PAUSED_TASK_NAME, CRABMATE_PLAN_REWRITE_NAME,
    CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME, CRABMATE_RUNTIME_ENV_SNAPSHOT_NAME,
    CRABMATE_WORKSPACE_CHANGELIST_NAME, Message, message_content_as_str,
};

/// 是否为 **`role: user`** 且非用户真实发言（编排注入、画像、记忆等）。
#[inline]
pub fn is_server_injected_user_message(m: &Message) -> bool {
    if m.role != "user" {
        return false;
    }
    if is_server_injected_user_by_name(m) {
        return true;
    }
    message_content_as_str(&m.content)
        .is_some_and(crate::cm_display_rules::user_message_should_hide_for_chat_display)
}

#[inline]
fn is_server_injected_user_by_name(m: &Message) -> bool {
    matches!(
        m.name.as_deref(),
        Some(CRABMATE_LONG_TERM_MEMORY_NAME)
            | Some(CRABMATE_WORKSPACE_CHANGELIST_NAME)
            | Some(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME)
            | Some(CRABMATE_FORCED_SKILL_NAME)
            | Some(CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME)
            | Some(CRABMATE_PLAN_REWRITE_NAME)
            | Some(CRABMATE_CONTEXT_SUMMARY_NAME)
            | Some(CRABMATE_PAUSED_TASK_NAME)
            | Some(CRABMATE_AUTO_RESUME_NAME)
            // `#1526` A1b：运行时环境快照——UI 不展示、落盘剥离，但**必须**发往模型，
            // 故只登记在这里，不登记进 `is_runtime_refreshed_injection`（见该函数注释）。
            | Some(CRABMATE_RUNTIME_ENV_SNAPSHOT_NAME)
    )
}

/// **由运行时每次 prepare 重新注入**的那一类（落盘里的旧副本必须剥离，否则会与
/// 最新状态各说一套）。这是新增注入类型时**唯一**需要登记的地方。
///
/// 与 [`is_server_injected_user_message`] 的区别：后者还包含**要发给模型**的注入
/// （首轮工作区画像、上下文摘要、plan rewrite 等），把它们一并剔除会改变模型输入。
pub fn is_runtime_refreshed_injection(m: &Message) -> bool {
    crate::cm_types::is_long_term_memory_injection(m)
        || crate::cm_types::is_workspace_changelist_injection(m)
        || crate::cm_types::is_paused_task_injection(m)
        // 自动续跑指令：每个续跑回合重新生成，落盘副本无意义（且会把「机器代发」写进历史）。
        || crate::cm_types::is_auto_resume_injection(m)
    // 注意：`/<id>` 强制技能正文（`CRABMATE_FORCED_SKILL_NAME`）**有意不登记在这里**。
    // 它虽然也是「每回合重建」，但重建点是 `turn_build`（直接写进 `seed.messages`），
    // 而不是出站前的 prepare 阶段。若登记为本类，
    // `messages_for_api_stripping_reasoning_skip_ui_separators` 会把正文从出站消息里剔除，
    // 而没有任何地方负责重新注入它 —— `/<id>` 就会静默失效（#1475）。
    // 它的落盘剥离由 `is_server_injected_user_message` 承担。
}

/// 落盘 / 快照前剥离「运行时刷新类」注入（LTM / 变更集 / 暂停任务摘要）。
///
/// 挂这里而不是各自实现：新增一类刷新型注入时只改 [`is_runtime_refreshed_injection`]，
/// 所有剥离点自动跟上。
pub fn strip_runtime_refreshed_injections(messages: &mut Vec<Message>) {
    messages.retain(|m| !is_runtime_refreshed_injection(m));
}

/// 落盘前剥离：编排类注入 user（保留首轮工作区画像、上下文摘要等仍须持久化的注入）。
pub fn strip_orchestration_injected_users_for_conversation_store(messages: &mut Vec<Message>) {
    messages.retain(|m| !should_strip_user_before_conversation_store(m));
}

#[inline]
fn should_strip_user_before_conversation_store(m: &Message) -> bool {
    m.role == "user"
        && is_server_injected_user_message(m)
        && !crate::cm_types::is_first_turn_workspace_context_injection(m)
        && !crate::cm_types::is_context_summary_injection(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::Message;

    /// `/<id>` 强制技能正文必须**出现在出站消息里**（否则该注入静默失效），
    /// 同时仍须在落盘前被剥离（由 [`is_server_injected_user_message`] 承担）。
    ///
    /// 回归守卫：#1475 最初把它登记进 [`is_runtime_refreshed_injection`]，结果
    /// `messages_for_api_stripping_reasoning_skip_ui_separators` 把它从出站消息里剔掉，
    /// 而没有任何地方负责重新注入 —— 正文根本发不到模型。
    #[test]
    fn forced_skill_survives_api_filter_but_is_persist_stripped() {
        let m = Message::user_forced_skill_injection("FORCED_BODY");
        assert!(
            is_server_injected_user_message(&m),
            "须被识别为服务端注入：UI 不展示、落盘剥离"
        );
        assert!(
            !is_runtime_refreshed_injection(&m),
            "不得登记为出站剔除类，否则正文发不到模型"
        );

        let api = crate::cm_types::messages_for_api_stripping_reasoning_skip_ui_separators(
            std::slice::from_ref(&m),
            false,
            false,
        );
        assert_eq!(api.len(), 1, "出站消息必须保留 forced skill 注入");

        let mut persisted = vec![m];
        strip_orchestration_injected_users_for_conversation_store(&mut persisted);
        assert!(persisted.is_empty(), "落盘前必须剥离");
    }

    /// 探针（`#1526` A1b「方向 A」可行性）：**只**登记 UI 隐藏、**不**登记
    /// `is_runtime_refreshed_injection` 的注入，必须能到达出站 body（模型可见），
    /// 同时落盘前仍被剥离。
    ///
    /// 与 [`forced_skill_survives_api_filter_but_is_persist_stripped`] 互补：那条用
    /// `messages_for_api_stripping_reasoning_skip_ui_separators`，这条走**真实出站函数**
    /// `conversation_messages_to_vendor_body`（`cm_llm::api::stream_chat` 的唯一入口），
    /// 因此与 `probe_runtime_refreshed_injection_reaches_vendor_body` 同口径可比。
    ///
    /// 断言以**内容到达**为主（`normalize` 可能合并相邻 user、丢掉 `name`），
    /// 这只影响归因，不影响「模型看不看得见」。
    /// `cm_llm` 只在 `server` feature 下存在；`scripts/check-client-contract.sh` 用
    /// `--no-default-features --features protocol` 编译 `cm_types` 的测试，故本断言须跟着收窄。
    /// 默认构建（本地 / CI 的 clippy·test job）仍会跑它。
    #[cfg(feature = "server")]
    #[test]
    fn probe_non_runtime_refreshed_injection_reaches_vendor_body() {
        let m = Message::user_forced_skill_injection("SNAPSHOT_PROBE_BODY");
        assert!(is_server_injected_user_message(&m), "须识别为服务端注入");
        assert!(
            !is_runtime_refreshed_injection(&m),
            "前置：不得登记 runtime_refreshed（否则出站被剔除）"
        );

        let messages = vec![Message::system_only("s"), m.clone()];
        let body =
            crate::cm_llm::conversation_messages_to_vendor_body(&messages, false, false, false);
        let joined = body
            .iter()
            .filter_map(|x| crate::cm_types::message_content_as_str(&x.content))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("SNAPSHOT_PROBE_BODY"),
            "正文应到达模型（实际 body={joined:?}）"
        );

        let mut persisted = vec![Message::user_only("hi"), m];
        strip_orchestration_injected_users_for_conversation_store(&mut persisted);
        assert_eq!(persisted.len(), 1, "落盘前必须剥离，不留副本");
    }
}

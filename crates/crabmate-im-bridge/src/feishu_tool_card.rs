//! 飞书 **交互卡片**：工具审批「允许一次 / 永久允许 / 拒绝」按钮（`msg_type: interactive`）。
//!
//! 按钮 **`value`** 使用固定键 **`crabmate_tool_decision`**，与 **`card.action.trigger`** 回调解析一致。
//! 卡片 JSON 采用常见 **1.x** 形态（`config` + `header` + `elements`）；若你方环境仅支持 2.0，请在搭建工具中自建模板并改用模板发送。
//!
//! **原地更新**：[`PATCH` 更新消息卡片](https://open.feishu.cn/document/uAjLw4CM/ukTMukTMukTM/reference/im-v1/message/patch) 要求卡片 **`config.update_multi: true`**（发送与更新后的 JSON 均需包含）。

use serde_json::{Value, json};

const VALUE_MARKER: &str = "crabmate_tool_decision";

fn card_config_patchable() -> Value {
    json!({
        "wide_screen_mode": true,
        "update_multi": true
    })
}

/// 开场占位卡片（可随后用 **`PATCH /im/v1/messages/:message_id`** 替换为结果摘要）。
pub fn progress_placeholder_card() -> Value {
    json!({
        "config": card_config_patchable(),
        "header": {
            "title": { "tag": "plain_text", "content": "CrabMate" },
            "template": "wathet"
        },
        "elements": [
            {
                "tag": "div",
                "text": {
                    "tag": "plain_text",
                    "content": "⏳ 正在执行…（完成后将在此卡片内更新摘要）"
                }
            }
        ]
    })
}

/// 构造 **`msg_type: interactive`** 的 **`content`** JSON（再经 `serde_json::to_string` 作为 API 的 `content` 字段）。
pub fn tool_approval_interactive_content(
    command: &str,
    args_preview: &str,
    approval_session_id: &str,
) -> Value {
    let body = format!(
        "待执行命令：{}\n参数摘要：{}\n审批会话：{}",
        truncate_plain(command, 200),
        truncate_plain(args_preview, 800),
        truncate_plain(approval_session_id, 128)
    );
    json!({
        "config": card_config_patchable(),
        "header": {
            "title": { "tag": "plain_text", "content": "CrabMate 工具审批" },
            "template": "orange"
        },
        "elements": [
            {
                "tag": "div",
                "text": {
                    "tag": "plain_text",
                    "content": body
                }
            },
            {
                "tag": "action",
                "actions": [
                    {
                        "tag": "button",
                        "text": { "tag": "plain_text", "content": "允许一次" },
                        "type": "primary",
                        "value": button_value("allow_once", approval_session_id)
                    },
                    {
                        "tag": "button",
                        "text": { "tag": "plain_text", "content": "永久允许" },
                        "type": "default",
                        "value": button_value("allow_always", approval_session_id)
                    },
                    {
                        "tag": "button",
                        "text": { "tag": "plain_text", "content": "拒绝" },
                        "type": "danger",
                        "value": button_value("deny", approval_session_id)
                    }
                ]
            },
            {
                "tag": "note",
                "elements": [
                    { "tag": "plain_text", "content": "点击按钮提交审批；亦可用 !允许一次 / !永久允许 / !拒绝。" }
                ]
            }
        ]
    })
}

fn truncate_plain(s: &str, max: usize) -> String {
    let t = s.trim().replace('\n', " ");
    if t.chars().count() <= max {
        t
    } else {
        t.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}

fn button_value(decision: &str, approval_session_id: &str) -> Value {
    json!({
        VALUE_MARKER: "1",
        "decision": decision,
        "approval_session_id": approval_session_id
    })
}

/// 从 **`card.action.trigger`**（schema 2.0）或旧版扁平载荷解析决策。
pub fn parse_card_tool_decision(v: &Value) -> Option<(String, String)> {
    let action = v.pointer("/event/action").or_else(|| v.get("action"))?;
    let value = action.get("value")?;
    let obj = value.as_object()?;
    if obj.get(VALUE_MARKER).and_then(|x| x.as_str()) != Some("1") {
        return None;
    }
    let decision = obj
        .get("decision")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_ascii_lowercase();
    let session = obj
        .get("approval_session_id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    Some((session, decision))
}

pub fn is_card_action_trigger_payload(v: &Value) -> bool {
    v.pointer("/header/event_type")
        .and_then(|x| x.as_str())
        .is_some_and(|t| t == "card.action.trigger")
        || v.get("type").and_then(|x| x.as_str()) == Some("card.action.trigger")
}

/// 卡片回调 HTTP 响应体：Toast（不返回 `card` 字段，避免格式校验问题）。
pub fn card_callback_ack_toast_zh(body: &str) -> Value {
    json!({
        "toast": {
            "type": "success",
            "i18n": {
                "zh_cn": body,
                "en_us": "OK"
            }
        }
    })
}

pub fn card_callback_error_toast_zh(body: &str) -> Value {
    json!({
        "toast": {
            "type": "error",
            "i18n": {
                "zh_cn": body,
                "en_us": "Error"
            }
        }
    })
}

/// 本轮结束后的**只读结果卡片**（无按钮）：标题 + 正文摘要；正文会先截断以防超过飞书卡片限制。
pub fn turn_result_card(title: &str, body: &str) -> Value {
    let body_safe = truncate_plain(body, 3500);
    json!({
        "config": card_config_patchable(),
        "header": {
            "title": { "tag": "plain_text", "content": truncate_plain(title, 100) },
            "template": "blue"
        },
        "elements": [
            {
                "tag": "div",
                "text": {
                    "tag": "plain_text",
                    "content": body_safe
                }
            },
            {
                "tag": "note",
                "elements": [
                    { "tag": "plain_text", "content": "长耗时任务期间仅显示本摘要；详细过程见 CrabMate 日志。" }
                ]
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_schema_v2_sample_shape() {
        let v = json!({
            "schema": "2.0",
            "header": { "event_type": "card.action.trigger" },
            "event": {
                "action": {
                    "value": {
                        "crabmate_tool_decision": "1",
                        "decision": "allow_once",
                        "approval_session_id": "feishu:om_x"
                    },
                    "tag": "button"
                }
            }
        });
        assert!(is_card_action_trigger_payload(&v));
        assert_eq!(
            parse_card_tool_decision(&v),
            Some(("feishu:om_x".into(), "allow_once".into()))
        );
    }

    #[test]
    fn turn_result_card_has_header() {
        let c = turn_result_card("完成", "hello");
        assert!(c.get("header").is_some());
        assert_eq!(
            c.pointer("/config/update_multi").and_then(|x| x.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn progress_placeholder_is_patchable() {
        let c = progress_placeholder_card();
        assert_eq!(
            c.pointer("/config/update_multi").and_then(|x| x.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn turn_result_card_truncates_long_body_in_builder() {
        let long = "a".repeat(5000);
        let c = turn_result_card("T", &long);
        let body = c
            .pointer("/elements/0/text/content")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        assert!(body.len() < long.len());
    }
}

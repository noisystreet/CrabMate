#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn url_verification_plain_challenge() {
        let v = json!({
            "type": "url_verification",
            "challenge": "abc123"
        });
        assert_eq!(url_verification_challenge(&v), Some("abc123".into()));
    }

    #[test]
    fn signature_skipped_when_no_header() {
        let cfg = FeishuBridgeConfig {
            app_id: "x".into(),
            app_secret: "y".into(),
            encrypt_key: Some("ek".into()),
            verify_signature_when_possible: true,
            verification_token: None,
            replay_timestamp_max_skew_secs: 0,
            nonce_dedup_ttl: Duration::ZERO,
            group_require_bot_mention: false,
            bot_open_id: None,
            crabmate: std::sync::Arc::new(
                CrabmateClient::new("http://127.0.0.1:9", "b").expect("client"),
            ),
            dedup_ttl: Duration::from_secs(1),
            max_message_content_json_chars: 12000,
            async_worker: false,
            event_queue_capacity: 1,
            workspace_root_template: None,
            tool_approval_mode: FeishuToolApprovalMode::DenyAll,
            tool_decision_secret: None,
            tool_decision_timeout_secs: 300,
            quiet_sse_status: false,
            result_card_max_body_chars: 3500,
            in_place_progress_card: false,
            event_queue_sqlite_path: None,
            sqlite_queue_max_retries: 5,
            sqlite_queue_poll_ms: 200,
            sqlite_queue_lease_secs: 600,
        };
        let headers = HeaderMap::new();
        assert!(!verify_lark_signature_if_needed(&cfg, &headers, "{}").unwrap());
    }

    #[test]
    fn parse_lark_ts_seconds_vs_millis() {
        assert_eq!(parse_lark_timestamp_secs("1600000000"), Some(1_600_000_000));
        assert_eq!(
            parse_lark_timestamp_secs("1600000000000"),
            Some(1_600_000_000)
        );
    }

    #[test]
    fn verification_token_ok() {
        let cfg = FeishuBridgeConfig {
            app_id: "x".into(),
            app_secret: "y".into(),
            encrypt_key: None,
            verify_signature_when_possible: false,
            verification_token: Some("vtok".into()),
            replay_timestamp_max_skew_secs: 0,
            nonce_dedup_ttl: Duration::ZERO,
            group_require_bot_mention: false,
            bot_open_id: None,
            crabmate: std::sync::Arc::new(
                CrabmateClient::new("http://127.0.0.1:9", "b").expect("client"),
            ),
            dedup_ttl: Duration::from_secs(1),
            max_message_content_json_chars: 12000,
            async_worker: false,
            event_queue_capacity: 1,
            workspace_root_template: None,
            tool_approval_mode: FeishuToolApprovalMode::DenyAll,
            tool_decision_secret: None,
            tool_decision_timeout_secs: 300,
            quiet_sse_status: false,
            result_card_max_body_chars: 3500,
            in_place_progress_card: false,
            event_queue_sqlite_path: None,
            sqlite_queue_max_retries: 5,
            sqlite_queue_poll_ms: 200,
            sqlite_queue_lease_secs: 600,
        };
        let v = json!({ "header": { "token": "vtok" } });
        assert!(verify_event_verification_token(&cfg, &v).is_ok());
    }

    #[test]
    fn mentions_detect_bot_open_id() {
        let m = json!({
            "mentions": [
                {
                    "mentioned_type": "bot",
                    "id": { "open_id": "ou_bot_1" }
                }
            ]
        });
        assert!(message_mentions_bot_open_id(&m, "ou_bot_1"));
        assert!(!message_mentions_bot_open_id(&m, "ou_other"));
    }

    #[test]
    fn split_for_result_card_splits_tail() {
        let s = "x".repeat(100);
        let (card, tail) = split_for_result_card(&s, 50);
        assert!(card.contains('…'));
        assert!(!tail.is_empty());
        assert!(card.chars().count() + tail.chars().count() >= s.chars().count());
    }
}

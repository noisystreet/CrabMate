// `finalize.rs` 的派生字段测试（include! 内联到 `finalize` 模块，可访问私有函数与 `use super::*`）。
// 独立成文件以保持 `finalize.rs` 行数在 fn-nloc 限值内。

mod background_job_defaults_tests {
    use super::*;

    #[test]
    fn background_job_fields_default_and_clamp() {
        let tr = derive_tool_registry_fields(&ConfigBuilder::default());
        assert!(!tr.tool_registry_background_jobs_enabled);
        assert_eq!(tr.tool_registry_background_job_max_concurrent, 4);
        assert_eq!(tr.tool_registry_background_job_max_queued, 32);
        assert_eq!(tr.tool_registry_background_job_ttl_secs, 86_400);
        assert_eq!(tr.tool_registry_background_job_result_grace_secs, 300);
        assert_eq!(tr.tool_registry_background_job_max_entries, 128);
        assert_eq!(tr.tool_registry_background_job_output_buffer_bytes, 262_144);

        let mut b = ConfigBuilder::default();
        b.tool_registry_policy.tool_registry_background_jobs_enabled = Some(true);
        b.tool_registry_policy.tool_registry_background_job_max_concurrent = Some(999);
        b.tool_registry_policy.tool_registry_background_job_ttl_secs = Some(1_000_000);
        b.tool_registry_policy.tool_registry_background_job_output_buffer_bytes = Some(30_000_000);
        let tr = derive_tool_registry_fields(&b);
        assert!(tr.tool_registry_background_jobs_enabled);
        assert_eq!(tr.tool_registry_background_job_max_concurrent, 256);
        assert_eq!(tr.tool_registry_background_job_ttl_secs, 604_800);
        assert_eq!(
            tr.tool_registry_background_job_output_buffer_bytes,
            16_777_216,
            "输出缓冲上限应钳制到 16 MiB"
        );
    }

    #[test]
    fn background_job_keys_parse_from_toml_and_finalize() {
        // 端到端：TOML 键名（serde 字段）→ apply_tool_registry → derive，防止字段名漂移静默落默认值。
        let sec: crate::cm_config::source::ToolRegistrySection = toml::from_str(
            r#"
background_jobs_enabled = true
background_job_max_concurrent = 6
background_job_max_queued = 64
background_job_ttl_secs = 7200
background_job_result_grace_secs = 120
background_job_max_entries = 256
background_job_output_buffer_bytes = 524288
"#,
        )
        .expect("parse [tool_registry] section");
        assert_eq!(sec.background_jobs_enabled, Some(true));
        assert_eq!(sec.background_job_max_concurrent, Some(6));
        assert_eq!(sec.background_job_max_queued, Some(64));
        assert_eq!(sec.background_job_ttl_secs, Some(7200));
        assert_eq!(sec.background_job_result_grace_secs, Some(120));
        assert_eq!(sec.background_job_max_entries, Some(256));
        assert_eq!(sec.background_job_output_buffer_bytes, Some(524288));

        let mut b = ConfigBuilder::default();
        b.apply_tool_registry(sec);
        let tr = derive_tool_registry_fields(&b);
        assert!(tr.tool_registry_background_jobs_enabled);
        assert_eq!(tr.tool_registry_background_job_max_concurrent, 6);
        assert_eq!(tr.tool_registry_background_job_max_queued, 64);
        assert_eq!(tr.tool_registry_background_job_ttl_secs, 7200);
        assert_eq!(tr.tool_registry_background_job_result_grace_secs, 120);
        assert_eq!(tr.tool_registry_background_job_max_entries, 256);
        assert_eq!(tr.tool_registry_background_job_output_buffer_bytes, 524_288);
    }

    #[test]
    fn tool_retry_fields_default_and_clamp() {
        let tr = derive_tool_registry_fields(&ConfigBuilder::default());
        assert!(!tr.tool_registry_tool_retry_enabled);
        assert_eq!(tr.tool_registry_tool_retry_max_attempts, 2);
        assert_eq!(tr.tool_registry_tool_retry_backoff_ms, 250);
        assert_eq!(
            tr.tool_registry_tool_retry_error_codes.as_ref(),
            &default_tool_retry_error_codes()
                .into_iter()
                .collect::<HashSet<_>>()
        );
        assert!(tr.tool_registry_tool_retry_denied_tools.is_empty());

        let mut b = ConfigBuilder::default();
        b.tool_registry_policy.tool_registry_tool_retry_enabled = Some(true);
        b.tool_registry_policy.tool_registry_tool_retry_max_attempts = Some(9);
        b.tool_registry_policy.tool_registry_tool_retry_backoff_ms = Some(20_000);
        b.tool_registry_policy.tool_registry_tool_retry_error_codes =
            Some(vec!["timeout".into(), " custom ".into()]);
        let tr = derive_tool_registry_fields(&b);
        assert!(tr.tool_registry_tool_retry_enabled);
        assert_eq!(tr.tool_registry_tool_retry_max_attempts, 5);
        assert_eq!(tr.tool_registry_tool_retry_backoff_ms, 10_000);
        assert_eq!(
            tr.tool_registry_tool_retry_error_codes.as_ref(),
            &["timeout".to_string(), "custom".to_string()]
                .into_iter()
                .collect::<HashSet<_>>()
        );
    }

    #[test]
    fn tool_retry_keys_parse_from_toml_and_finalize() {
        // 端到端：TOML 键名（serde 字段）→ apply_tool_registry → derive，防止字段名漂移静默落默认值。
        let sec: crate::cm_config::source::ToolRegistrySection = toml::from_str(
            r#"
tool_retry_enabled = true
tool_retry_max_attempts = 3
tool_retry_backoff_ms = 500
tool_retry_error_codes = ["timeout", "http_timeout"]
tool_retry_denied_tools = ["http_fetch"]
"#,
        )
        .expect("parse [tool_registry] section");
        assert_eq!(sec.tool_retry_enabled, Some(true));
        assert_eq!(sec.tool_retry_max_attempts, Some(3));
        assert_eq!(sec.tool_retry_backoff_ms, Some(500));
        assert_eq!(
            sec.tool_retry_error_codes,
            Some(vec!["timeout".to_string(), "http_timeout".to_string()])
        );
        assert_eq!(
            sec.tool_retry_denied_tools,
            Some(vec!["http_fetch".to_string()])
        );

        let mut b = ConfigBuilder::default();
        b.apply_tool_registry(sec);
        let tr = derive_tool_registry_fields(&b);
        assert!(tr.tool_registry_tool_retry_enabled);
        assert_eq!(tr.tool_registry_tool_retry_max_attempts, 3);
        assert_eq!(tr.tool_registry_tool_retry_backoff_ms, 500);
        assert!(tr
            .tool_registry_tool_retry_error_codes
            .contains("http_timeout"));
        assert!(tr
            .tool_registry_tool_retry_denied_tools
            .contains("http_fetch"));
    }
}

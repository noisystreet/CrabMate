    use super::*;

    /// #1340：`--llm-context-tokens` 必须在 user-data 覆盖**之后**落定，不能被它盖回。
    ///
    /// 测试直接模拟「user-data 已把窗口写成大值」这一步，再应用 CLI 覆盖：
    /// 顺序一旦被改回 user-data 在后，这条断言就会红。
    #[test]
    fn cli_llm_context_tokens_override_wins_over_user_data_layer() {
        let mut cfg = crate::cm_config::load_config(None).expect("embedded default config");
        cfg.llm_sampling.llm_context_tokens = 1_000_000;
        apply_cli_llm_context_tokens_override(&mut cfg, Some(100_000));
        assert_eq!(cfg.llm_sampling.llm_context_tokens, 100_000);
    }

    /// 未给 CLI 覆盖时，user-data 的值必须原样保留（不能顺手清成配置默认）。
    #[test]
    fn absent_cli_override_keeps_user_data_value() {
        let mut cfg = crate::cm_config::load_config(None).expect("embedded default config");
        cfg.llm_sampling.llm_context_tokens = 1_000_000;
        apply_cli_llm_context_tokens_override(&mut cfg, None);
        assert_eq!(cfg.llm_sampling.llm_context_tokens, 1_000_000);
    }

    /// `0` 视为「不覆盖」（见 `GlobalOpts::llm_context_tokens` 的说明），上限仍是 10M。
    #[test]
    fn cli_override_is_capped_at_ten_million() {
        let mut cfg = crate::cm_config::load_config(None).expect("embedded default config");
        apply_cli_llm_context_tokens_override(&mut cfg, Some(u32::MAX));
        assert_eq!(cfg.llm_sampling.llm_context_tokens, 10_000_000);
    }

    /// 层表驱动的代价是「这一层到底写不写」被藏进闭包：这里钉住 `apply_layer` 的 CLI 臂。
    ///
    /// 顺序由 golden 守，本用例守「还在不在」——删掉 Cli 那一行，它立刻变红。
    #[test]
    fn cli_layer_writes_through_apply_layer_once() {
        let mut cfg = crate::cm_config::load_config(None).expect("embedded default config");
        cfg.llm_sampling.llm_context_tokens = 1_000_000;
        let mut cli_tokens = Some(262_144);
        apply_layer(&mut cfg, ConfigLayer::Cli, &mut cli_tokens);
        assert_eq!(cfg.llm_sampling.llm_context_tokens, 262_144);
        assert_eq!(cli_tokens, None, "CLI 覆盖只消费一次");

        cfg.llm_sampling.llm_context_tokens = 1_000_000;
        apply_layer(&mut cfg, ConfigLayer::Cli, &mut cli_tokens);
        assert_eq!(
            cfg.llm_sampling.llm_context_tokens, 1_000_000,
            "覆盖已被消费，第二次不应再生效"
        );
    }

    /// UserData 臂读本机 user-data 目录（磁盘/钥匙串依赖），这里只断言它**不消费** CLI 覆盖：
    /// 具体取值由 `layer_precedence` 的 golden 用注入值覆盖，避免测试随本机文件飘。
    #[test]
    fn user_data_layer_leaves_cli_override_untouched() {
        let mut cfg = crate::cm_config::load_config(None).expect("embedded default config");
        let mut cli_tokens = Some(123_456);
        apply_layer(&mut cfg, ConfigLayer::UserData, &mut cli_tokens);
        assert_eq!(cli_tokens, Some(123_456));
        assert_ne!(cfg.llm_sampling.llm_context_tokens, 0);
    }

    /// 内嵌默认（1024 条 × 262,144 窗口）满足双闸不变量：启动不应刷告警。
    #[test]
    fn embedded_default_config_has_no_gate_warning() {
        let cfg = crate::cm_config::load_config(None).expect("embedded default config");
        assert!(
            context_gate_warning(&cfg).is_none(),
            "默认配置不该告警：{:?}",
            context_gate_warning(&cfg)
        );
    }

    /// #1342 验收 1 的内核：自检输入必须是**层序都应用之后**的最终 cfg。
    ///
    /// 场景取自 Issue 实测：1M 窗口 + 256 条告警（307,200 < 834,332）；
    /// 同一配置再经 CLI 层把窗口收成 262,144 后告警消失（307,200 > 207,155）。
    /// 若把 CLI 覆盖挪回 user-data 之前，第二段断言就会红。
    #[test]
    fn gate_warning_tracks_final_cfg_after_cli_layer() {
        let mut cfg = crate::cm_config::load_config(None).expect("embedded default config");
        cfg.session_ui.max_message_history = 256;
        cfg.llm_sampling.llm_context_tokens = 1_000_000;
        cfg.llm_sampling.max_tokens = 16_384;
        assert!(
            context_gate_warning(&cfg).is_some(),
            "1M 窗口 + 256 条应告警"
        );
        apply_cli_llm_context_tokens_override(&mut cfg, Some(262_144));
        assert!(
            context_gate_warning(&cfg).is_none(),
            "CLI 层收小窗口后条数闸不再先到，不该告警"
        );
    }

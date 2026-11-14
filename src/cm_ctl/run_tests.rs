//! `cm_ctl::run`（CLI 分派）的单元测试。
//!
//! 单独成文件：`run.rs` 逼近 920 行上限（`scripts/fn_nloc_rust_metrics.py`），
//! 拆测试是保持该上限的手段；测试路径仍是 `cm_ctl::run::tests`。

    use super::*;
    use crate::cm_ctl::paths::CtlPaths;
    use crate::cm_ctl::port_pool::PortPool;
    use crate::cm_ctl::supervise::{StartSpec, Step, plan};
    use std::collections::BTreeMap;

    fn cmd(name: Option<&str>) -> StartCmd {
        StartCmd {
            name: name.map(str::to_string),
            all: false,
            instances_file: None,
            dry_run: true,
            serve_config: None,
            port: None,
            yes: false,

            env_file: None,
            timeout_secs: 10,
        }
    }

    fn ctx_at(dir: &std::path::Path) -> Ctx {
        Ctx {
            paths: CtlPaths::for_test(dir),
            pool: PortPool::default_pool(),
        }
    }

    fn start(name: &str) -> Step {
        Step::Start {
            name: name.to_string(),
            spec: StartSpec {
                workspace: Some("/tmp/nope".to_string()),
                serve_config: None,
                port: None,
                env: BTreeMap::new(),
                env_file_env: CredEnv::default(),
                profile: None,
                profile_fingerprint: None,
                no_tools: false,
            },
            restart: false,
            reason: "测试".to_string(),
        }
    }

    /// 既没给名字、也没给 `--all`：必须是用法错误，而不是「起了一个空名字的实例」。
    #[test]
    fn up_without_name_or_all_is_missing_name() {
        let e = start_args(&cmd(None), None).expect_err("应报错");
        assert_eq!(e.error_code, "missing_name");
    }

    #[test]
    fn all_with_instance_name_is_conflicting_flags() {
        let mut c = cmd(Some("proj-a"));
        c.all = true;
        assert_eq!(
            reject_name_with_all(&c).expect_err("应报错").error_code,
            "conflicting_flags"
        );
    }

    /// 单实例参数在批量模式下必须报错：静默忽略会让人以为 `--port` 生效了。
    #[test]
    fn all_rejects_single_instance_flags() {
        let mut c = cmd(None);
        c.port = Some(8080);
        assert_eq!(
            reject_single_instance_flags(&c).expect_err("应报错").error_code,
            "conflicting_flags"
        );

        let mut c = cmd(None);
        c.yes = true;
        assert_eq!(
            reject_single_instance_flags(&c).expect_err("应报错").error_code,
            "conflicting_flags"
        );

        // 都不给时不得报错（批量模式本来就该零参数跑）
        assert!(reject_single_instance_flags(&cmd(None)).is_ok());
    }

    /// 只有 `Start` 进入执行集合：回收是 `supervise` 的事。
    #[test]
    fn starts_only_drops_everything_but_start() {
        let steps = vec![
            Step::Running { name: "a".into() },
            start("b"),
            Step::Stop {
                name: "c".into(),
                reason: "测试".into(),
            },
            Step::Skipped {
                name: "d".into(),
                reason: "测试".into(),
            },
        ];
        let kept = starts_only(steps);
        assert_eq!(kept.len(), 1, "只应保留 Start：{kept:?}");
        assert!(matches!(&kept[0], Step::Start { name, .. } if name == "b"));
    }

    /// 声明里 `enabled = false` 的实例不能被 `up --all` 启动
    /// （`plan` 把它判成 `Skipped`，而这里只留 `Start`）。
    #[test]
    fn plan_filter_excludes_disabled_declaration() {
        let dir = std::env::temp_dir().join(format!(
            "cm_ctl_up_all_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("config")).expect("建目录");
        let ctx = ctx_at(&dir);
        let decl = dir.join("config").join("instances.toml");
        std::fs::write(
            &decl,
            "[instances.on]\nworkspace = \"/tmp/nope-on\"\n\n\
             [instances.off]\nworkspace = \"/tmp/nope-off\"\nenabled = false\n",
        )
        .expect("写声明");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&decl, std::fs::Permissions::from_mode(0o600)).expect("权限");

        let file = instances::load(&decl).expect("读声明");
        let kept = starts_only(plan(&ctx, &file).expect("计划"));
        assert!(
            kept.iter()
                .any(|s| matches!(s, Step::Start { name, .. } if name == "on")),
            "启用的实例应进入启动集合：{kept:?}"
        );
        assert!(
            !kept
                .iter()
                .any(|s| matches!(s, Step::Start { name, .. } if name == "off")),
            "disabled 的实例不该被启动：{kept:?}"
        );
    }

    fn scoped_start(name: &str) -> supervise::Step {
        supervise::Step::Start {
            name: name.to_string(),
            spec: supervise::StartSpec {
                workspace: Some("/tmp/nope".to_string()),
                serve_config: None,
                port: None,
                env: std::collections::BTreeMap::new(),
                env_file_env: CredEnv::default(),
                profile: None,
                profile_fingerprint: None,
                no_tools: false,
            },
            restart: false,
            reason: "测试".to_string(),
        }
    }

    /// 限定范围时，**别的实例的步骤**（尤其 `Stop` / 孤儿回收）必须一起被丢掉：
    /// 「守护这一个」不能变成「顺手停掉没让我停的东西」。
    #[test]
    fn scoped_drops_other_instances_including_stops() {
        let steps = vec![
            scoped_start("a"),
            supervise::Step::Stop {
                name: "b".to_string(),
                reason: "声明中已不存在".to_string(),
            },
            supervise::Step::Running {
                name: "c".to_string(),
            },
        ];
        let kept = supervise::scoped(steps, Some("a"));
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!(matches!(&kept[0], supervise::Step::Start { name, .. } if name == "a"));
    }

    /// 目标实例**自己的** `Stop` 要留下：声明判给它的（如 `enabled = false`）仍是事实。
    #[test]
    fn scoped_keeps_target_stop_and_none_keeps_all() {
        let steps = vec![
            supervise::Step::Stop {
                name: "a".to_string(),
                reason: "声明为 enabled = false".to_string(),
            },
            supervise::Step::Stop {
                name: "b".to_string(),
                reason: "声明中已不存在".to_string(),
            },
        ];
        let kept = supervise::scoped(steps.clone(), Some("a"));
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!(matches!(&kept[0], supervise::Step::Stop { name, .. } if name == "a"));

        // `None` = 整份声明：默认行为必须与引入范围之前逐字一致
        assert_eq!(supervise::scoped(steps, None).len(), 2);
    }

    /// 范围裁剪必须作用在**真实计划**上，而不只是那个纯函数：加范围时第一版漏了
    /// 一次性收敛这条路径（它当时直接调 `plan`），`supervise alpha --dry-run` 照旧
    /// 列出了 `beta`。这条测试钉住的就是「路径有没有接上唯一入口」。
    #[test]
    fn scoped_plan_on_real_plan_excludes_other_instances() {
        let dir = std::env::temp_dir().join(format!(
            "cm_ctl_scopeplan_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("config")).expect("建目录");
        let ctx = ctx_at(&dir);
        let decl = dir.join("config").join("instances.toml");
        std::fs::write(
            &decl,
            "[instances.a]\nworkspace = \"/tmp/nope-a\"\n\n\
             [instances.b]\nworkspace = \"/tmp/nope-b\"\n",
        )
        .expect("写声明");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&decl, std::fs::Permissions::from_mode(0o600)).expect("权限");

        let file = instances::load(&decl).expect("读声明");
        let mut opts = one_shot_opts(&cmd(None));
        opts.scope = Some("a".to_string());
        let kept = supervise::scoped_plan(&ctx, &file, &opts).expect("计划");
        assert!(
            kept.iter()
                .any(|s| matches!(s, supervise::Step::Start { name, .. } if name == "a")),
            "目标实例应在计划里：{kept:?}"
        );
        assert!(
            kept.iter().all(|s| s.name() != "b"),
            "范围外的实例不得出现在计划里：{kept:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 单实例监督只认声明：名字不在 `instances.toml` 里必须报 `not_declared`
    /// （记录里不存 env 值，无法可靠重建启动规格 —— 见 `ensure_declared` 的注释）。
    #[test]
    fn ensure_declared_rejects_unknown_name() {
        let dir = std::env::temp_dir().join(format!(
            "cm_ctl_scope_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("config")).expect("建目录");
        let decl = dir.join("config").join("instances.toml");
        std::fs::write(&decl, "[instances.demo]\nworkspace = \"/tmp/nope-demo\"\n").expect("写声明");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&decl, std::fs::Permissions::from_mode(0o600)).expect("权限");

        // 省略名字 = 整份声明，不做名字校验
        assert!(ensure_declared(&decl, None).is_ok());
        // 声明里有的名字可以
        assert!(ensure_declared(&decl, Some("demo")).is_ok());
        // 声明里没有 → 用法错误，而不是「计划为空但你不知道为什么」
        let e = ensure_declared(&decl, Some("nope")).expect_err("应报错");
        assert_eq!(e.error_code, "not_declared");
        assert!(e.message.contains("nope"), "{}", e.message);

        let _ = std::fs::remove_dir_all(&dir);
    }

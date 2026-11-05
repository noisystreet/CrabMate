//! `serve` 子命令：装配 HTTP 宿主并跑到底（S5 第二步，见设计 `docs/design/sdk_facade.md` §6）。
//!
//! **装配仍在根包侧**（本模块随 `cm_cli` 编译于根包）：`cm_web_host` 被禁边不许引用
//! `crate::cm_internal`，而 `AppState` 的 handler 因 axum `FromRef` 孤儿规则必须在根包。
//! 因此宿主模块只提供 builder / 路由层，装配产物（`Arc<AppState>`）由这里注入。
//!
//! 需要 `cm_internal` 的两处（后台任务注册表 + TTL 清理定时器）经
//! `crate::build_serve_tool_job_registry` 门面转一手，保持 `cm_cli/` 无 `cm_internal` 引用。

mod auth;
mod lock;
mod paths;
mod runtime_state;
mod store;

use std::net::SocketAddr;
use std::sync::Arc;

use log::info;

use crate::cm_config::cli::CliGlobals;
use crate::cm_config::cli::definitions::ServeCmd;
use crate::web;

use self::runtime_state::ServeRuntimeBuildInput;

pub(super) async fn run(
    session: super::session::CliSessionStart,
    global: &CliGlobals,
    serve: &ServeCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let super::session::CliSessionStart {
        cfg_holder,
        client,
        tools,
        process_handles,
        api_key,
    } = session;
    let cfg_holder = &cfg_holder;

    // 加锁必须在打开/迁移会话库与长期记忆库之前；锁保留到函数结束 = 覆盖整个进程生命周期。
    let paths::ServePrecheck {
        instance_id,
        paths,
        instance_locks: _instance_locks,
    } = paths::serve_precheck(
        cfg_holder,
        global.workspace.as_deref(),
        global.config_path.as_deref(),
    )
    .await?;

    let state = runtime_state::build(ServeRuntimeBuildInput {
        cfg_holder,
        config_path: &global.config_path,
        client,
        tools,
        api_key,
        initial_workspace: paths::initial_workspace_override(
            &global.workspace,
            serve.workspace_unset,
        ),
        process_handles,
        paths,
    })
    .await?;

    let sched_tasks = {
        let g = cfg_holder.read().await;
        g.conversation_persistence.scheduled_agent_tasks.clone()
    };
    web::cron_scheduler::spawn_serve_cron_scheduler(Arc::clone(&state), sched_tasks);
    auth::serve_require_web_api_bearer_when_enabled(cfg_holder).await?;
    let web_api_bearer_layer_enabled = auth::serve_web_api_bearer_layer_enabled(cfg_holder).await;
    let cors_allowed_origins = {
        let g = cfg_holder.read().await;
        g.web_api.web_cors_allowed_origins.clone()
    };
    let app = web::server::build_app(
        state.clone(),
        web_api_bearer_layer_enabled,
        cors_allowed_origins.clone(),
    );

    let http_bind_host = resolve_http_bind_host(serve.host.as_deref());
    let bind_ip = parse_bind_ip(&http_bind_host)?;
    let auth_enabled = auth::serve_validate_bind_gates(cfg_holder, bind_ip).await?;
    let addr = SocketAddr::from((bind_ip, serve.bind_port()));
    auth::serve_log_startup_health(cfg_holder, &global.workspace).await;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_addr = listener.local_addr()?;
    println!("Web 服务已启动");
    println!("  监听: http://{}/", actual_addr);
    serve_log_bind_warnings(bind_ip, auth_enabled, actual_addr);
    auth::serve_log_cors_startup(&cors_allowed_origins);
    if serve.desktop_ready_json {
        print_desktop_ready_json(
            actual_addr,
            web_api_bearer_layer_enabled,
            instance_id.as_deref(),
            global.config_path.as_deref(),
        );
    }
    info!(target: "crabmate", "Web 服务监听 addr={}", actual_addr);
    runtime_state::spawn_uploads_cleanup(state.clone());
    runtime_state::spawn_conversation_prune(state.clone());
    crate::web::spawn_tiktoken_baseline_warmup(state.clone());

    // 优雅关闭：监听 SIGTERM / SIGINT，构建 axum graceful shutdown 信号
    let shutdown_signal = build_serve_shutdown_signal(Arc::clone(&state));

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal)
    .await?;
    Ok(())
}

/// `serve` 监听地址：`--host`、`CM_HTTP_HOST`、默认 `127.0.0.1`。
///
/// 派生量放在 serve 分支而不是 `ParsedCliArgs` 字段（S5，见 #1295）：它是「我们怎么解释
/// 参数」，不是「用户给了什么」。
fn resolve_http_bind_host(host_opt: Option<&str>) -> String {
    host_opt
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("CM_HTTP_HOST")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

/// `--http-bind-host` 解析；错误信息直接面向用户，故保留原文。
fn parse_bind_ip(http_bind_host: &str) -> Result<std::net::IpAddr, std::io::Error> {
    http_bind_host.parse().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "无效的 Web 监听地址 {:?}（请使用有效 IP，如 127.0.0.1 或 0.0.0.0）",
                http_bind_host
            ),
        )
    })
}

/// 启动期把「实际监听地址 + 是否启用鉴权」讲清楚：不配 token 不是错误，但必须显式提示。
fn serve_log_bind_warnings(bind_ip: std::net::IpAddr, auth_enabled: bool, actual_addr: SocketAddr) {
    if bind_ip.is_unspecified() && !auth_enabled {
        eprintln!(
            "  警告: 正在监听所有网卡（{}），接口无鉴权，请勿在不可信网络暴露",
            actual_addr
        );
    }
    if bind_ip.is_loopback() && !auth_enabled {
        eprintln!(
            "  提示: 未配置 web_api_bearer_token 时，受保护路由不校验 Bearer（见中间件逻辑）；对外或共享网络请设置 CM_WEB_API_BEARER_TOKEN / web_api_bearer_token / `crabmate web-bearer set`，并可将 web_api_require_bearer=true 强制启动前须配密钥。浏览器须在设置中保存同一串 Web API 共享密钥（localStorage「crabmate-api-bearer-token」；勿与模型 API_KEY 混淆）。"
        );
    }
    if bind_ip.is_unspecified() && auth_enabled {
        println!("  安全: 已启用 Web API 鉴权（Authorization: Bearer 或 X-API-Key）");
    }
}

/// `--web-ready-json` / `--desktop-ready-json`：给宿主进程（Desktop / `crabmate ctl`）的单行 JSON。
fn print_desktop_ready_json(
    actual_addr: SocketAddr,
    web_api_bearer_layer_enabled: bool,
    instance_id: Option<&str>,
    config: Option<&str>,
) {
    let ready = paths::serve_ready_json(
        actual_addr,
        web_api_bearer_layer_enabled,
        instance_id,
        config,
    );
    println!("{}", serde_json::to_string(&ready).unwrap_or_default());
}

/// 构建 axum graceful shutdown 信号：监听 SIGTERM/SIGINT（第二次 SIGINT 强退）→ 关闭 ChatJobQueue → 等待 in-flight 完成（最多 30 秒）。
fn build_serve_shutdown_signal(
    state: Arc<crate::AppState>,
) -> impl std::future::Future<Output = ()> + Send {
    let shutdown = crate::shutdown::GracefulShutdown::new();
    shutdown.clone().spawn_signal_handler();

    let graceful = shutdown.clone();
    async move {
        graceful.wait_for_shutdown().await;
        state.chat.chat_queue.shutdown();
        log::info!(target: "crabmate", "等待队列运行中任务完成...");
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let remaining = state.chat.chat_queue.running_count();
        if remaining > 0 {
            log::warn!(target: "crabmate", "优雅关闭超时，仍有 {} 个任务正在运行", remaining);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_bind_ip, resolve_http_bind_host};

    #[test]
    fn bind_host_falls_back_to_loopback() {
        // 显式空/空白一律视为未给，回落到默认回环地址（除非进程设了 CM_HTTP_HOST）。
        if std::env::var("CM_HTTP_HOST").is_err() {
            assert_eq!(resolve_http_bind_host(None), "127.0.0.1");
            assert_eq!(resolve_http_bind_host(Some("  ")), "127.0.0.1");
        }
        assert_eq!(resolve_http_bind_host(Some("0.0.0.0")), "0.0.0.0");
    }

    #[test]
    fn bad_bind_host_message_is_user_facing() {
        let err = parse_bind_ip("not-an-ip").expect_err("非 IP 应被拒绝");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("无效的 Web 监听地址"));
    }
}

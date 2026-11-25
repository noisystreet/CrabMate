//! `execute` 模块单元测试（拆出以降低 `execute.rs` 物理行数棘轮）。

use super::super::meta::HandlerLookupTable;
use super::*;
// #1198：子模块化后，测试专用项直接从对应子模块取（不再靠 `include!` 的同作用域裸名）。
use super::dispatch::read_dir_path_is_external;
use super::run_command::{
    ExecMode, ParsedRunCommandJson, PolicyContext, PolicyError, RunCommandHostInvoke,
    emit_run_command_tool_output_chunk, evaluate_policy, execute_run_command_impl,
    parse_run_command_json, posix_shell_wrap_needs_interactive_approval,
};
use std::path::Path;
use std::sync::Arc;

use crate::cm_types::{CommandApprovalDecision, FunctionCall, ToolCall};

fn tool_call(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: "tc_1".to_string(),
        typ: "function".to_string(),
        function: FunctionCall {
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
    }
}

#[test]
fn read_dir_path_is_external_detects_absolute_and_parent_ref() {
    assert_eq!(
        read_dir_path_is_external(r#"{"path":"/tmp"}"#),
        Some("/tmp".to_string())
    );
    assert_eq!(
        read_dir_path_is_external(r#"{"path":"../secrets"}"#),
        Some("../secrets".to_string())
    );
    assert_eq!(read_dir_path_is_external(r#"{"path":"src"}"#), None);
}

#[tokio::test]
async fn prefetch_parallel_syncdefault_approvals_blocks_external_read_dir_without_channel() {
    let calls = vec![tool_call("read_dir", r#"{"path":"/tmp"}"#)];
    let failures = prefetch_parallel_syncdefault_approvals(
        &calls,
        None,
        &HandlerLookupTable::default_dispatch(),
        None,
    )
    .await;
    assert_eq!(failures.len(), 1);
    let msg = failures
        .get(&("read_dir".to_string(), r#"{"path":"/tmp"}"#.to_string()))
        .expect("missing failure for external read_dir");
    assert!(msg.contains("需要审批通道"));
}

#[tokio::test]
async fn prefetch_http_fetch_blocks_without_channel_when_prefix_miss() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.http_fetch.http_fetch_allowed_prefixes.clear();
    let cfg = Arc::new(cfg);
    let args = r#"{"url":"https://example.com/private"}"#;
    let calls = vec![tool_call("http_fetch", args)];
    let failures = prefetch_http_fetch_parallel_approvals(&calls, &cfg, None, None).await;
    assert_eq!(failures.len(), 1);
    let msg = failures
        .get(&("http_fetch".to_string(), args.to_string()))
        .expect("missing failure for http_fetch");
    assert!(msg.contains("http_fetch_allowed_prefixes"));
    assert!(msg.contains("审批通道"));
}

#[tokio::test]
async fn prefetch_http_fetch_skips_approval_when_prefix_matches() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.http_fetch.http_fetch_allowed_prefixes = vec!["https://example.com/".to_string()];
    let cfg = Arc::new(cfg);
    let args = r#"{"url":"https://example.com/api/v1"}"#;
    let calls = vec![tool_call("http_fetch", args)];
    let failures = prefetch_http_fetch_parallel_approvals(&calls, &cfg, None, None).await;
    assert!(failures.is_empty());
}

pub(super) fn parsed_run_command(args_json: &str) -> ParsedRunCommandJson {
    parse_run_command_json(args_json, std::path::Path::new(".")).expect("parse run_command json")
}

pub(super) fn test_policy_context<'a>(
    cfg: &'a Arc<AgentConfig>,
    web_ctx: Option<&'a dyn crate::cm_approval::ApprovalBroker>,
    mode: ExecMode,
) -> PolicyContext<'a> {
    PolicyContext {
        cfg,
        workspace: std::path::Path::new("."),
        web_ctx,
        mode,
        sse_command: "run_command",
        permission: None,
    }
}

/// 带权限能力快照的策略上下文（S3 等级行为用例）。`permission = None` 的现状用例仍用
/// [`test_policy_context`]，两者刻意分开，避免「忘传 caps」被默认值掩盖。
pub(super) fn test_policy_context_with_caps<'a>(
    cfg: &'a Arc<AgentConfig>,
    permission: &'a dyn crate::cm_agent::permission::PolicyEngine,
    web_ctx: Option<&'a dyn crate::cm_approval::ApprovalBroker>,
    mode: ExecMode,
) -> PolicyContext<'a> {
    PolicyContext {
        cfg,
        workspace: std::path::Path::new("."),
        web_ctx,
        mode,
        sse_command: "run_command",
        permission: Some(permission),
    }
}

/// 同 [`test_policy_context_with_caps`]，但可指定工作目录。
///
/// S7-cmd（命令轴根集合）用例需要真实的 `session` 目录：`"."` 下「根内 / 根外」失去区分度，
/// 且 `../sibling` 会指到仓库外面去。
pub(super) fn test_policy_context_with_caps_at<'a>(
    cfg: &'a Arc<AgentConfig>,
    permission: &'a dyn crate::cm_agent::permission::PolicyEngine,
    web_ctx: Option<&'a dyn crate::cm_approval::ApprovalBroker>,
    mode: ExecMode,
    workspace: &'a std::path::Path,
) -> PolicyContext<'a> {
    PolicyContext {
        cfg,
        workspace,
        web_ctx,
        mode,
        sse_command: "run_command",
        permission: Some(permission),
    }
}

/// 测试专用的 owned 评估结果。
///
/// R3 后 [`PolicyDecision`] 只**借用** [`RunCommandCaps`]（借用生命周期绑不住
/// `evaluate_policy` 的返回值），逐用例声明快照会把噪音铺满矩阵测试；这里把借用结果
/// 折叠成一份 owned 快照，断言形态与改造前一致（`d.allowed_commands` / `d.skip_arg_safety`）。
#[derive(Debug)]
pub(super) struct EvaluatedPolicy {
    pub(super) allowed_commands: Arc<[String]>,
    pub(super) skip_arg_safety: bool,
}

/// 包一层 [`evaluate_policy`]：内部持有快照，对外回 owned 结果。
pub(super) async fn evaluate_for_test(
    parsed: &ParsedRunCommandJson,
    ctx: &PolicyContext<'_>,
) -> Result<EvaluatedPolicy, PolicyError> {
    let mut policy_caps = ctx.command_caps();
    let decision = evaluate_policy(parsed, ctx, &mut policy_caps).await?;
    Ok(EvaluatedPolicy {
        allowed_commands: Arc::clone(&decision.caps.allowed_commands),
        skip_arg_safety: decision.skip_arg_safety,
    })
}

/// R3 不变量：快照只在**成功**路径末尾一次性写回。
///
/// 失败用例验证「中途可能已发生扩展但整体 `Err`」时快照保持原值——`AllowOnce` 的临时
/// 批准不该泄漏到调用方的快照里；成功用例验证写回确实生效（否则引用方读到的永远是基白名单）。
#[tokio::test]
async fn caps_snapshot_only_updated_on_success() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    // 窄白名单且不含 `bash`，保证探针命令必然走审批分支。
    cfg.command_exec.allowed_commands = vec!["cargo".to_string()].into();
    let cfg = Arc::new(cfg);
    let cmd = "crabmate_r3_caps_probe";
    let parsed = parsed_run_command(&format!(r#"{{"command":"{cmd}","args":[]}}"#));

    // 失败：无审批通道 → unknown-cmd gate 直接 Err。
    let ctx = test_policy_context(&cfg, None, ExecMode::Sync);
    let mut policy_caps = ctx.command_caps();
    let (msg, _) = evaluate_policy(&parsed, &ctx, &mut policy_caps)
        .await
        .expect_err("no approval channel")
        .into_message();
    assert!(msg.contains("审批通道不可用"), "{msg}");
    assert_eq!(
        policy_caps.allowed_commands.as_ref(),
        cfg.command_exec.allowed_commands.as_ref(),
        "失败路径不应改写快照"
    );

    // 成功：AllowOnce 批准 → 扩展结果写回同一快照。
    let fixture = approval_fixture(CommandApprovalDecision::AllowOnce);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let mut policy_caps = ctx.command_caps();
    evaluate_policy(&parsed, &ctx, &mut policy_caps)
        .await
        .expect("allow once");
    assert!(
        contains_cmd(&policy_caps.allowed_commands, cmd),
        "{:?}",
        policy_caps.allowed_commands
    );
}

#[tokio::test]
async fn posix_shell_wrap_skips_glob_when_bash_allowlisted() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allowed_commands = vec!["ls".to_string(), "bash".to_string()].into();
    let cfg = Arc::new(cfg);
    let parsed = parsed_run_command(r#"{"command":"ls","args":["*.rs"]}"#);

    let ctx = test_policy_context(&cfg, None, ExecMode::Sync);
    let d = evaluate_for_test(&parsed, &ctx)
        .await
        .expect("glob + bash → no extra approval");
    assert!(!d.skip_arg_safety);
}

#[tokio::test]
async fn posix_shell_wrap_skips_cd_prefix_without_expansion() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allowed_commands = vec!["cd".into(), "git".into(), "bash".into()].into();
    let cfg = Arc::new(cfg);
    let parsed = parsed_run_command(r#"{"command":"cd","args":["src","&&","git","status"]}"#);

    let ctx = test_policy_context(&cfg, None, ExecMode::Sync);
    evaluate_for_test(&parsed, &ctx)
        .await
        .expect("cd peel → git status");
}

#[test]
fn posix_shell_wrap_web_operators_need_approval_even_with_bash() {
    assert!(posix_shell_wrap_needs_interactive_approval(
        "ls",
        &["&&".to_string(), "pwd".to_string()],
        true,
        true,
    ));
    assert!(!posix_shell_wrap_needs_interactive_approval(
        "ls",
        &["&&".to_string(), "pwd".to_string()],
        true,
        false,
    ));
    assert!(!posix_shell_wrap_needs_interactive_approval(
        "ls",
        &["*.rs".to_string()],
        true,
        true,
    ));
}

/// 带审批通道的 Web 运行时；预置决策后可由 gate 直接消费。
pub(super) struct ApprovalFixture {
    pub(super) web: crate::cm_tools::tool_runtime::WebToolRuntime,
    _out_rx: tokio::sync::mpsc::Receiver<String>,
}

pub(super) fn approval_fixture(decision: CommandApprovalDecision) -> ApprovalFixture {
    use tokio::sync::{Mutex as TokioMutex, mpsc};
    let (out_tx, out_rx) = mpsc::channel::<String>(8);
    let (approval_tx, approval_rx) = mpsc::channel::<CommandApprovalDecision>(8);
    approval_tx.try_send(decision).expect("preseed decision");
    let web = crate::cm_tools::tool_runtime::WebToolRuntime {
        out_tx,
        approval_rx_shared: Arc::new(TokioMutex::new(approval_rx)),
        approval_request_guard: Arc::new(TokioMutex::new(())),
        persistent_allowlist_shared: Arc::new(TokioMutex::new(std::collections::HashSet::new())),
        turn_job_id: Some(1),
        cancel: None,
    };
    ApprovalFixture {
        web,
        _out_rx: out_rx,
    }
}

pub(super) fn contains_cmd(allowed: &[String], cmd: &str) -> bool {
    allowed.iter().any(|c| c.eq_ignore_ascii_case(cmd))
}

#[tokio::test]
async fn unknown_cmd_gate_decision_matrix() {
    use CommandApprovalDecision::{AllowAlways, AllowOnce, Deny};

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    // `load_config` 会读环境覆盖：外部 shell 若设了 `CM_ALLOWED_COMMANDS=*`（本地跑 `serve` 常见），
    // 「该命令不在白名单」的前提就失效，断言会与外部环境绑定。这里固定成窄白名单，
    // 且**不含 `bash`**——下面的断言要验证「批准后把 bash 纳入生效白名单」。
    cfg.command_exec.allowed_commands = vec!["cargo".to_string()].into();
    let cfg = Arc::new(cfg);
    let cmd = "crabmate_unknown_gate_cmd";
    // `$HOME` 触发 shell 包装需求，批准后应同时把 bash 纳入生效白名单。
    let parsed = parsed_run_command(&format!(r#"{{"command":"{cmd}","args":["$HOME"]}}"#));

    let ctx = test_policy_context(&cfg, None, ExecMode::Sync);
    let (msg, _) = evaluate_for_test(&parsed, &ctx)
        .await
        .expect_err("no channel")
        .into_message();
    assert!(msg.contains("审批通道不可用"), "{msg}");

    let ctx = test_policy_context(&cfg, None, ExecMode::Async);
    let (msg, _) = evaluate_for_test(&parsed, &ctx)
        .await
        .expect_err("async")
        .into_message();
    assert!(msg.contains("async"), "{msg}");

    let fixture = approval_fixture(Deny);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let (msg, _) = evaluate_for_test(&parsed, &ctx)
        .await
        .expect_err("deny")
        .into_message();
    assert!(msg.contains("用户拒绝执行命令"), "{msg}");

    let fixture = approval_fixture(AllowOnce);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let d = evaluate_for_test(&parsed, &ctx).await.expect("allow once");
    assert!(
        contains_cmd(&d.allowed_commands, cmd),
        "{:?}",
        d.allowed_commands
    );
    assert!(
        contains_cmd(&d.allowed_commands, "bash"),
        "{:?}",
        d.allowed_commands
    );

    let fixture = approval_fixture(AllowAlways);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let d = evaluate_for_test(&parsed, &ctx)
        .await
        .expect("allow always");
    assert!(
        contains_cmd(&d.allowed_commands, cmd),
        "{:?}",
        d.allowed_commands
    );
    assert!(
        contains_cmd(&d.allowed_commands, "bash"),
        "{:?}",
        d.allowed_commands
    );
    // `test_policy_context` 不带 `caps`（`permission = None`），所以这里断言的是
    // 「未接线路径写裸键」——与引入等级前的行为一致。若将来该 fixture 接上 caps，
    // 断言须改成 `scoped_allowlist_key(Some(level), cmd)`。
    assert!(
        fixture
            .web
            .persistent_allowlist_shared
            .lock()
            .await
            .contains(cmd),
        "AllowAlways should persist the command"
    );
}

#[tokio::test]
async fn shell_wrap_gate_decision_matrix() {
    use CommandApprovalDecision::{AllowAlways, AllowOnce, Deny};

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allowed_commands = vec!["echo".to_string()].into();
    let cfg = Arc::new(cfg);
    let parsed = parsed_run_command(r#"{"command":"echo","args":["$HOME"]}"#);

    let fixture = approval_fixture(AllowOnce);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let d = evaluate_for_test(&parsed, &ctx).await.expect("allow once");
    assert!(
        contains_cmd(&d.allowed_commands, "bash"),
        "{:?}",
        d.allowed_commands
    );

    let fixture = approval_fixture(AllowAlways);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let d = evaluate_for_test(&parsed, &ctx)
        .await
        .expect("allow always");
    assert!(
        contains_cmd(&d.allowed_commands, "bash"),
        "{:?}",
        d.allowed_commands
    );

    let fixture = approval_fixture(Deny);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let (msg, _) = evaluate_for_test(&parsed, &ctx)
        .await
        .expect_err("deny")
        .into_message();
    assert!(msg.contains("已拒绝"), "{msg}");

    let fixture = approval_fixture(AllowOnce);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Async);
    let (msg, _) = evaluate_for_test(&parsed, &ctx)
        .await
        .expect_err("async")
        .into_message();
    assert!(msg.contains("async"), "{msg}");
}

#[tokio::test]
async fn external_path_gate_decision_matrix() {
    use CommandApprovalDecision::{AllowAlways, AllowOnce, Deny};

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allow_external_path_with_approval = true;
    let cfg = Arc::new(cfg);
    let parsed = parsed_run_command(r#"{"command":"cat","args":["/etc/passwd"]}"#);

    let fixture = approval_fixture(AllowOnce);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let d = evaluate_for_test(&parsed, &ctx).await.expect("allow once");
    assert!(d.skip_arg_safety);

    let fixture = approval_fixture(AllowAlways);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let d = evaluate_for_test(&parsed, &ctx)
        .await
        .expect("allow always");
    assert!(d.skip_arg_safety);

    let fixture = approval_fixture(Deny);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let (msg, _) = evaluate_for_test(&parsed, &ctx)
        .await
        .expect_err("deny")
        .into_message();
    assert!(msg.contains("已拒绝"), "{msg}");

    let fixture = approval_fixture(AllowOnce);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Async);
    let (msg, _) = evaluate_for_test(&parsed, &ctx)
        .await
        .expect_err("async")
        .into_message();
    assert!(msg.contains("async"), "{msg}");
}

#[test]
fn run_command_chunk_seq_is_monotonic_on_mirror() {
    use crate::cm_sse_protocol::sse::protocol::SsePayload;
    use crate::cm_tools::subprocess_session::SessionStream;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;

    let seen = Arc::new(Mutex::new(Vec::<u64>::new()));
    let seen_cb = Arc::clone(&seen);
    let mirror: crate::cm_sse_protocol::sse::SseControlMirror = Arc::new(move |p| {
        if let SsePayload::ToolOutputChunk { tool_output_chunk } = p {
            seen_cb.lock().expect("lock").push(tool_output_chunk.seq);
        }
    });
    let seq = AtomicU64::new(0);
    let utf8 = Mutex::new(Vec::<u8>::new());
    assert!(emit_run_command_tool_output_chunk(
        &seq,
        "tc-p1",
        SessionStream::Stdout,
        b"a\n",
        None,
        Some(&mirror),
        &utf8,
    ));
    assert!(emit_run_command_tool_output_chunk(
        &seq,
        "tc-p1",
        SessionStream::Stderr,
        b"e\n",
        None,
        Some(&mirror),
        &utf8,
    ));
    assert_eq!(*seen.lock().expect("lock"), vec![1, 2]);
}

#[tokio::test]
async fn run_command_chunk_try_send_full_does_not_bump_seq() {
    use crate::cm_tools::subprocess_session::SessionStream;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;
    use tokio::sync::mpsc;

    let seq = AtomicU64::new(0);
    let utf8 = Mutex::new(Vec::<u8>::new());
    let (tx, _rx) = mpsc::channel::<String>(1);
    tx.try_send("fill".into()).expect("fill");
    assert!(!emit_run_command_tool_output_chunk(
        &seq,
        "tc-p1",
        SessionStream::Stdout,
        b"lost?\n",
        Some(&tx),
        None,
        &utf8,
    ));
    assert_eq!(seq.load(std::sync::atomic::Ordering::SeqCst), 0);
}

pub(super) fn async_test_sandbox() -> Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend> {
    crate::tool_sandbox::default_sync_default_sandbox_backend()
}

pub(super) fn async_test_env<'a>(
    cfg: &'a Arc<AgentConfig>,
    sandbox: &'a Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend>,
) -> ToolExecEnv<'a> {
    ToolExecEnv {
        cfg,
        sandbox_backend: sandbox,
        permission: None,
    }
}

/// 经装配入口（workspace 检查 → 能力快照 → 解析 → 审批 → façade）执行 `run_command`。
pub(super) async fn run_command_impl_for_test<'a>(
    env: &'a ToolExecEnv<'a>,
    wd: &'a Path,
    web_ctx: Option<&'a dyn crate::cm_approval::ApprovalBroker>,
    registry: Option<Arc<crate::jobs::tool_jobs::ToolJobRegistry>>,
    args: &'a str,
) -> (String, Option<serde_json::Value>) {
    let mut workspace_changed = false;
    execute_run_command_impl(RunCommandHostInvoke {
        env,
        effective_working_dir: wd,
        workspace_is_set: true,
        workspace_changed: &mut workspace_changed,
        web_ctx,
        args,
        tool_call_id: "tc_run_command_test",
        sse_out_tx: None,
        sse_control_mirror: None,
        cancel: None,
        tool_jobs: registry,
    })
    .await
}

#[tokio::test]
async fn run_command_async_disabled_returns_invalid_args() {
    // 总开关默认已为 `true`，此处显式关闭以验证「关闭时返回 invalid_args」语义。
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.tool_registry_policy
        .tool_registry_background_jobs_enabled = false;
    let cfg = Arc::new(cfg);
    let sandbox = async_test_sandbox();
    let env = async_test_env(&cfg, &sandbox);
    let wd = std::path::Path::new(".");
    let (out, inject) = run_command_impl_for_test(
        &env,
        wd,
        None,
        None,
        r#"{"command":"echo","args":["hi"],"async":true}"#,
    )
    .await;
    assert!(out.contains("未启用"), "{out}");
    assert!(inject.is_none());
    drop(sandbox);
}

#[tokio::test]
async fn run_command_async_enabled_creates_job_and_returns_start_frame() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.tool_registry_policy
        .tool_registry_background_jobs_enabled = true;
    let cfg = Arc::new(cfg);
    let registry = crate::jobs::tool_jobs::registry_from_config(&cfg);
    let sandbox = async_test_sandbox();
    let env = async_test_env(&cfg, &sandbox);
    let wd = std::path::Path::new(".");
    let (out, inject) = run_command_impl_for_test(
        &env,
        wd,
        None,
        Some(Arc::clone(&registry)),
        r#"{"command":"echo","args":["async-hi"],"async":true,"timeout_secs":5}"#,
    )
    .await;
    let tj = inject
        .as_ref()
        .and_then(|v| v.get("tool_job"))
        .expect("tool_job 启动帧");
    let id = tj.get("tool_job_id").and_then(|x| x.as_str()).expect("id");
    assert!(id.starts_with("tooljob_"));
    assert_eq!(
        tj.get("tool_job_status").and_then(|x| x.as_str()),
        Some("queued")
    );
    assert_eq!(
        tj.get("tool_job_poll_url").and_then(|x| x.as_str()),
        Some(format!("/tools/jobs/{id}").as_str())
    );
    assert!(out.contains(id), "{out}");
    // 等待 worker 完成（async 已启动），校验注册表落定终态。
    let id_owned = id.to_string();
    loop {
        let rec = registry.get(&id_owned).expect("record");
        if rec.status.is_terminal() {
            assert_eq!(rec.status, crate::jobs::tool_jobs::JobStatus::Succeeded);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    drop(sandbox);
}

/// 发起路径须把 `web_ctx.turn_job_id` 写入 `JobRecord.source_turn_job_id`，
/// 使 `POST /chat/stream/{job_id}/cancel` 能级联取消本回合尚未终态的后台任务。
#[tokio::test]
async fn run_command_async_records_source_turn_job_id_and_cascades_cancel() {
    use crate::cm_types::CommandApprovalDecision;
    use tokio::sync::{Mutex as TokioMutex, mpsc};

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.tool_registry_policy
        .tool_registry_background_jobs_enabled = true;
    // `sleep` 不在默认白名单；加入以让长任务在取消前保持非终态（避免触发审批路径）。
    let mut allowed: Vec<String> = cfg.command_exec.allowed_commands.to_vec();
    allowed.push("sleep".to_string());
    cfg.command_exec.allowed_commands = allowed.into();
    let cfg = Arc::new(cfg);
    let registry = crate::jobs::tool_jobs::registry_from_config(&cfg);
    let sandbox = async_test_sandbox();
    let env = async_test_env(&cfg, &sandbox);
    let wd = std::path::Path::new(".");

    // 构造带 `turn_job_id` 的 Web 运行时（`out_tx` 保持存活以模拟连接未关闭）。
    let (out_tx, _out_rx) = mpsc::channel::<String>(8);
    let (_approval_tx, approval_rx) = mpsc::channel::<CommandApprovalDecision>(1);
    let web_ctx = crate::cm_tools::tool_runtime::WebToolRuntime {
        out_tx,
        approval_rx_shared: Arc::new(TokioMutex::new(approval_rx)),
        approval_request_guard: Arc::new(TokioMutex::new(())),
        persistent_allowlist_shared: Arc::new(TokioMutex::new(std::collections::HashSet::new())),
        turn_job_id: Some(4242),
        cancel: None,
    };

    // 用长睡眠命令确保任务在取消前仍处于非终态。
    let (out, inject) = run_command_impl_for_test(
        &env,
        wd,
        Some(&web_ctx),
        Some(Arc::clone(&registry)),
        r#"{"command":"sleep","args":["30"],"async":true,"timeout_secs":60}"#,
    )
    .await;
    let id = inject
        .as_ref()
        .and_then(|v| v.get("tool_job"))
        .and_then(|tj| tj.get("tool_job_id"))
        .and_then(|x| x.as_str())
        .expect("tool_job_id")
        .to_string();
    assert!(out.contains(&id), "{out}");

    // 记录须带来源回合 id。
    let rec = registry.get(&id).expect("record");
    assert_eq!(rec.source_turn_job_id, Some(4242));

    // 级联取消：本回合（4242）应命中该任务。
    assert_eq!(registry.cancel_non_terminal_for_source_turn(4242), 1);
    // 其它回合不命中。
    assert_eq!(registry.cancel_non_terminal_for_source_turn(9999), 0);

    // 等待 worker 落定终态（cancelled）。
    loop {
        let rec = registry.get(&id).expect("record");
        if rec.status.is_terminal() {
            assert_eq!(rec.status, crate::jobs::tool_jobs::JobStatus::Cancelled);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    drop(sandbox);
}

#[tokio::test]
async fn run_command_async_rejects_allow_once_requiring_command() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.tool_registry_policy
        .tool_registry_background_jobs_enabled = true;
    // 同 `unknown_cmd_gate_decision_matrix`：外部 `CM_ALLOWED_COMMANDS=*` 会让「未知命令」
    // 变成放行、断言失真；固定成窄白名单（`definitely_not_allowlisted_cmd_xyz` 不在其中）。
    cfg.command_exec.allowed_commands = vec!["cargo".to_string()].into();
    let cfg = Arc::new(cfg);
    let registry = crate::jobs::tool_jobs::registry_from_config(&cfg);
    let sandbox = async_test_sandbox();
    let env = async_test_env(&cfg, &sandbox);
    let wd = std::path::Path::new(".");
    // `definitely_not_allowlisted_cmd_xyz` 不在白名单且无审批通道（web_ctx=None）→ 拒绝 async。
    let (out, inject) = run_command_impl_for_test(
        &env,
        wd,
        None,
        Some(Arc::clone(&registry)),
        r#"{"command":"definitely_not_allowlisted_cmd_xyz","async":true}"#,
    )
    .await;
    assert!(out.contains("AllowAlways"), "{out}");
    assert!(inject.is_none());
    assert_eq!(registry.stats().total, 0, "拒绝后不得创建任务");
    drop(sandbox);
}

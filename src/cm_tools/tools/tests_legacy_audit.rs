//! Legacy 工具的「静默成功」审计。
//!
//! [`ToolRunner::Legacy`] 的成败由 [`parse_legacy_output`] 从正文推断，判据只有
//! `run_command.rs::looks_like_failure` 那六条：
//!
//! ```text
//! starts_with("错误") | starts_with("未知工具") | starts_with("参数解析错误")
//! | starts_with("执行失败") | contains("失败") | contains("超时")
//! ```
//!
//! 实现层若用别的措辞报错 —— `参数错误：…`（首字不是「错误」）、`未找到 X：…`、
//! `不支持的格式: …`、`路径无法解析: …` —— 编排层就会拿到 `ok: true`，模型被告知
//! 「成功」。`reminder` / `event` 的四个分支与 `archive_*` 的「不支持的归档格式」
//! 都是这么埋着的。
//!
//! 本模块**直接调用 `spec.runner` 的实现函数**，绕开 `run_tool_dispatch` 的 JSON
//! Schema 校验层：那层对 Legacy 也生效，会把「缺必填字段」提前拦成 `invalid_args`，
//! 从而掩盖实现自身的措辞问题。这里要审的正是实现层。
//!
//! 维护约定：
//! - 新增 Legacy 工具 ⇒ `CASES` 必须补一条（`legacy_audit_table_covers_every_legacy_tool` 会拦）。
//! - 把某工具迁成 `Typed` ⇒ 它的条目会因「已非 Legacy」而报错，顺手删掉。
//! - 修好一个 `KnownBug` ⇒ 挪进 `Fails`，同时把 `KNOWN_BUG_BUDGET` 减一。

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::tests::{test_allowed_commands, test_ctx};
use super::*;
use crate::cm_tools::tool_result::parse_legacy_output;

/// 已知「实现报错但被判成功」的工具数快照。每修好一个就减一 —— 这个数字就是进度条。
///
/// 已从 **4 → 0**：`background_job` / `background_job_cancel`（`参数错误：` 开头）与
/// `workflow_execute`（同步面占位句）迁 `Typed` 后清掉三个，最后一个 `rust_analyzer`
/// 也随 LSP 族迁 `Typed` 修掉（`路径无法解析: …` 与 `… : LSP error: …` 两处静默成功）。
/// 归零后本表只保留 `Fails` / `Legal` 两档；再出现 `KnownBug` 说明又开了新洞。
const KNOWN_BUG_BUDGET: usize = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// 实现层必然失败 ⇒ 必须被判为失败。
    Fails,
    /// **已知缺陷**：实现在报错，但措辞不含判定关键词 ⇒ 现在被判成功。
    KnownBug,
    /// 合法成功：入参虽不合语义，实现选择容忍或跳过，本身不算失败。
    ///
    /// 表内目前已无 Legacy 用例命中这一档 —— 只读聚合三兄弟迁 `Typed` 后，它们的
    /// `Legal` 语义搬进了各自单测。保留变体是因为三档结构要能表达这种情况，
    /// 下一个「容忍非法入参」的 Legacy 工具进来时直接用它。
    #[allow(dead_code)]
    Legal,
}

/// 每个 Legacy 工具一条「语义非法、但结构合法」的入参。
///
/// 选参原则：只踩**实现层自己的校验与查找失败**，不触发网络、子进程编译、
/// 真实测试执行等副作用（绕开 schema 校验后，`{}` 可能直接走到 `unwrap`）。
const CASES: &[(&str, &str, Expect, &str)] = &[
    // ── 外部 API 族（`get_weather` / `web_search`）与受控 HTTP 族（`http_fetch` / `http_request`）
    //    均已迁 Typed（2026-10-28）—— 原先在这里靠「city 太短」「无 API Key」「非法 scheme」
    //    这类正文关键词判定，现在由各自单测按 `error_code` 断言：
    //    `weather::tests`、`web_search::tests` 的 provider 分支、`http_fetch/tests.rs` 的
    //    `run_direct` / `run_request_direct`（非法 scheme、白名单拒绝）──
    // ── LSP：`rust_analyzer` 已迁 Typed（失败一律显式 `Err(ToolError)`），
    //    其错误码划分与 8 个 action 的失败分支由 `rust_ide::tests` 直接覆盖，不再经本表 ──
    // ── 控制面 ──────────────────────────────────────────────────
    (
        "present_clarification_questionnaire",
        r#"{}"#,
        Expect::Fails,
        "缺 questionnaire_id / intro / questions（当前连正文都缺，靠「退出码：1」兜底）",
    ),
    // ── 诊断 / 聚合：`diagnostic_summary` / `repo_overview_sweep` /
    //    `crate_contract_map` 均已迁 Typed（2026-10-28，D 档）—— 它们原先在这里按
    //    `Legal` 钉住「非法字段只降级」，现在那条语义由各自 `*_tolerates_*` 用例守，
    //    不再经本表 ──
    (
        "docs_health_sweep",
        r#"{"md_roots":"not-an-array"}"#,
        Expect::Fails,
        "「参数解析错误: invalid type …」",
    ),
    (
        "codebase_semantic_search",
        r#"{"query":""}"#,
        Expect::Fails,
        "test_ctx 未注入语义检索宿主",
    ),
    // ── 工作流：`workflow_execute` 已迁 Typed（同步面返回 `Err`），
    //    见 `runners::runner_workflow_execute_try` ──
    // ── Python 族：`pytest_run` / `python_snippet_run` 均已迁 Typed（失败显式 `Err(ToolError)`），
    //    错误码由 `python_tools::tests` 覆盖，不再经本表 ──
    // ── Git / GitHub 族：`gh_*` 五个与 `git` 均已迁 Typed（失败显式 `Err(ToolError)`），
    //    码见 `mod.rs` 的 `command_not_allowed` / `gh_{子命令}_failed` / `git_{action}_failed`，
    //    由各文件单测覆盖（`git::read_ops::tests`），不再经本表 ──
    // ── 后台任务：`background_job` / `background_job_cancel` 均已迁 Typed，
    //    其错误码由 `background_job_tools` 的单元测试覆盖，不再经本表 ──
    // ── 导航 ────────────────────────────────────────────────────
    (
        "code_nav",
        r#"{"action":"find_symbol"}"#,
        Expect::Fails,
        "`find_symbol` 需要非空 symbol",
    ),
];

/// 取某 Legacy 工具的实现函数；已迁 `Typed` / 不存在时返回 `None`。
fn legacy_runner(name: &str) -> Option<fn(&str, &ToolContext<'_>) -> String> {
    match tool_specs().iter().find(|s| s.name == name)?.runner {
        ToolRunner::Legacy(f) => Some(f),
        _ => None,
    }
}

#[test]
fn legacy_tools_do_not_report_silent_success() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let mut bugs = 0usize;

    for (name, args, expect, reason) in CASES {
        let f = legacy_runner(name).unwrap_or_else(|| {
            panic!("{name} 已不是 Legacy（{reason}）—— 迁移完成后请删掉本表条目")
        });
        let body = catch_unwind(AssertUnwindSafe(|| f(args, &ctx)))
            .unwrap_or_else(|_| panic!("{name} 在入参 {args} 上 panic（{reason}）"));
        let ok = parse_legacy_output(name, &body).ok;

        match expect {
            Expect::Fails => assert!(
                !ok,
                "{name} 静默成功（{reason}）：正文以「{}」开头，不含 looks_like_failure 关键词",
                body.lines().next().unwrap_or("")
            ),
            Expect::Legal => assert!(ok, "{name} 本应容忍该入参却报失败（{reason}）：{body}"),
            Expect::KnownBug => {
                bugs += 1;
                assert!(
                    ok,
                    "{name} 已不再静默成功（{reason}）—— 请把这条挪进 Expect::Fails，\
                     并把 KNOWN_BUG_BUDGET 减一：{body}"
                );
            }
        }
    }

    assert_eq!(
        bugs, KNOWN_BUG_BUDGET,
        "KnownBug 条目数（{bugs}）与 KNOWN_BUG_BUDGET（{KNOWN_BUG_BUDGET}）不一致：\
         修复后请同步下调预算"
    );
}

/// 表必须覆盖全部 Legacy 工具：新增 Legacy 工具时这个测试会提醒补用例。
#[test]
fn legacy_audit_table_covers_every_legacy_tool() {
    for spec in tool_specs()
        .iter()
        .filter(|s| matches!(s.runner, ToolRunner::Legacy(_)))
    {
        assert!(
            CASES.iter().any(|(name, ..)| *name == spec.name),
            "`{}` 是 Legacy 但审计表里没有用例 —— 请补一条「语义非法、结构合法」的入参",
            spec.name
        );
    }
}

/// 探针：打印每个工具的实际判定与正文首行，用于定迁移优先级。
///
/// ```text
/// cargo test legacy_audit_probe -- --ignored --nocapture
/// ```
#[test]
#[ignore = "审计探针：cargo test legacy_audit_probe -- --ignored --nocapture"]
fn legacy_audit_probe() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let legacy = tool_specs()
        .iter()
        .filter(|s| matches!(s.runner, ToolRunner::Legacy(_)))
        .count();
    println!("\n=== Legacy 工具 {legacy} 个 / 用例 {} 条 ===", CASES.len());

    for (name, args, expect, _) in CASES {
        let Some(f) = legacy_runner(name) else {
            println!("{name:<34} SKIP  (已非 Legacy)");
            continue;
        };
        match catch_unwind(AssertUnwindSafe(|| f(args, &ctx))) {
            Ok(body) => {
                let parsed = parse_legacy_output(name, &body);
                let first: String = body.lines().next().unwrap_or("").chars().take(64).collect();
                println!("{name:<34} ok={:<5} {expect:?} | {first}", parsed.ok);
            }
            Err(_) => println!("{name:<34} PANIC  {expect:?}"),
        }
    }
    println!();
}

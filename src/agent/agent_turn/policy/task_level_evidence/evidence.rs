//! 完成判定的**第 ① 层：结构化证据采集**（#1387 S0.5）。
//!
//! 把多次工具调用归一为带稳定 id 的**结构化事实**，每条只陈述「发生了什么」，
//! **不清言任务是否完成**——那是第 ② 层（LLM 语义判定）的职责。
//!
//! ## 为什么需要这一层
//!
//! 旧实现把「事实采集」与「完成判定」压在同一批关键词判断里，导致两个方向的错误：
//!
//! 1. **判定依赖工具名**：`generic_tool_is_build_or_test_success` 前置
//!    `tool_name == run_command`，于是用规范的 `cargo_test` 反而判不出——越规范越失效；
//! 2. **强度不可区分**：`run_command` 跑 `make` 只能靠 `built target` 等文本特征（弱证据），
//!    而 `cargo_test` 带结构化 `exit_code`（强证据），旧实现把两者同等对待。
//!
//! 本层把这两件事拆开：**证据来自哪个工具、有多硬**，与「够不够算完成」互不耦合。
//!
//! ## 口径
//!
//! - [`EvidenceKind::Verify`]：验证性执行。**专用工具**（`cargo_test` 等）走结构化退出码，
//!   `run_command` 则必须靠输出特征识别，因而强度天然更弱。
//! - [`EvidenceKind::Write`]：改动工作区文件。
//! - [`EvidenceKind::Other`]：其余（含只读探查）。
//!
//! `Verify` / `Write` 的名单**不是** `cm_config::tool_write_effect` 的副本：那份是「有副作用」
//! 的宽口径（含 `http_request` / `reminder` 等不改工作区的工具），本层要的是两个**更窄**的
//! 语义——「改过工作区」与「验证过结果」。两处口径不同，不应合并。

use crate::cm_names as tool_names;
use crate::cm_tools::tool_result::{normalize_tool_message_content, parse_tool_output};
use crate::cm_types::{Message, message_content_as_str};

/// 一次工具调用贡献的证据种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    /// 验证性执行（构建、测试、静态检查）。
    Verify,
    /// 改动工作区文件。
    Write,
    /// 其余（只读探查、网络、调度等）。
    Other,
}

impl EvidenceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verify => "verify",
            Self::Write => "write",
            Self::Other => "other",
        }
    }
}

/// 证据强度：决定「能否据此提前停轮」。
///
/// 这是本层存在的核心理由——旧的布尔 `build_or_test_success` 无法表达
/// 「同一件事，一次是结构化退出码、一次是文本猜测」这个差距。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceStrength {
    /// 结构化事实：工具自身的 `exit_code` / 信封 `ok`，不需要文本推断。
    Strong,
    /// 文本特征推断：从输出里认出「built target」「test result: ok」之类标记。
    Weak,
}

impl EvidenceStrength {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::Weak => "weak",
        }
    }
}

/// 一条结构化证据。`id` 稳定且有序（`ev_1`…），供第 ② 层判定时回引。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceItem {
    pub id: String,
    pub kind: EvidenceKind,
    /// `Write` / `Other` 也填：写盘成功同样是结构化事实。
    pub strength: EvidenceStrength,
    pub tool: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    /// 截断后的输出片段，供判定层与日志引用。
    pub detail: String,
}

/// 输出片段保留的字符上限（避免证据本身把上下文撑爆）。
const DETAIL_MAX_CHARS: usize = 200;

/// 一次回合采集到的全部证据。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvidenceSet {
    items: Vec<EvidenceItem>,
}

impl EvidenceSet {
    /// 从消息历史采集证据。
    ///
    /// 只认 `role == "tool"` 的消息：证据必须来自工具执行，`assistant` 自述**不入账**
    /// （旧实现的 `assistant_completion_claim` 正是把自述当证据的地方）。
    #[must_use]
    pub fn collect(messages: &[Message]) -> Self {
        let mut items = Vec::new();
        for m in messages {
            if m.role != "tool" {
                continue;
            }
            let Some(raw) = message_content_as_str(&m.content) else {
                continue;
            };
            let name = m.name.as_deref().unwrap_or("");
            if let Some(item) = evidence_from_tool_message(name, raw, items.len()) {
                items.push(item);
            }
        }
        Self { items }
    }

    #[must_use]
    pub fn items(&self) -> &[EvidenceItem] {
        &self.items
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

}

/// 验证性执行的**专用**工具：自身输出带结构化退出码，无需文本推断。
///
/// 名单全部取自 [`crate::cm_names`]（内建工具名单一真源，勿写字面量）。
fn is_dedicated_verify_tool(name: &str) -> bool {
    matches!(
        name,
        tool_names::CARGO_TEST | tool_names::CARGO_CHECK | tool_names::CARGO_CLIPPY
            | tool_names::CARGO_RUN
            | tool_names::PYTEST_RUN
    )
}

/// 改动工作区文件的工具（**窄口径**：只算真正写盘的，不含执行类）。
///
/// 与 `cm_config::tool_write_effect` 的区别是刻意的：那份把 `run_command` / `cargo_test` /
/// `http_request` / `reminder` 都算「有副作用」，而本层只回答「工作区被改过吗」。
fn is_workspace_write_tool(name: &str) -> bool {
    matches!(
        name,
        tool_names::CREATE_FILE
            | tool_names::MODIFY_FILE
            | tool_names::APPEND_FILE
            | tool_names::APPLY_PATCH
            | tool_names::SEARCH_REPLACE
            | tool_names::COPY_FILE
            | tool_names::MOVE_FILE
            | tool_names::DELETE_FILES
            | tool_names::DELETE_DIR
            | tool_names::CREATE_DIR
    )
}

/// `run_command` 是否算「验证性执行」。
///
/// **结构化优先**：`structured_payload.invocation` 是完整命令行，按「程序名 + 子命令」判定，
/// 不看输出文本。**文本兜底**：无信封的旧消息（没有 `invocation`）才退回看输出特征。
///
/// 为什么要有两段（#1401 #19）：输出文本两个方向都会错——`make` 是 `make sure …` 的子串，
/// 而 `ninja` / `cargo nextest` / `go test` 的输出里根本没有 `built target`。命令本身是结构化
/// 字段，「这次跑的是不是构建/测试」直接看它更准，不必从输出反推。
fn run_command_is_verify(invocation: Option<&str>, text_lower: &str) -> bool {
    match invocation {
        Some(inv) if !inv.trim().is_empty() => command_line_looks_like_verify(inv),
        _ => run_command_output_looks_like_verify(text_lower),
    }
}

/// 命令行是否在跑构建 / 测试。
///
/// 按「每条子命令的首个程序名 + 紧跟的动词」判定，而不是整串 `contains`：
/// `cd build && cargo test` 会被切段后各自判定。
///
/// **残余风险**：把真实命令藏在脚本文件里（`./build.sh`）识别不到，会落 `Other`——
/// 漏词方向只损失早停机会，不产出假证据。
///
/// `pub(crate)`：#1422 A5 之后 `outer_loop_build_idle` 的「构建进展」判定也读同一个函数，
/// 避免「同一语义两套实现」重新长出来。
pub(crate) fn command_line_looks_like_verify(invocation: &str) -> bool {
    command_line_looks_like_verify_inner(invocation, 0)
}

/// shell 嵌套包装的递归深度上限（`bash -c "bash -c ..."`）。
const MAX_SHELL_WRAP_DEPTH: usize = 3;

fn command_line_looks_like_verify_inner(invocation: &str, depth: usize) -> bool {
    if depth > MAX_SHELL_WRAP_DEPTH {
        return false;
    }
    let lower = invocation.to_lowercase();
    // 切段：`&&` / `||` / `;` / 换行，每段是一条独立命令。
    lower
        .split(['&', '|', ';', '\n'])
        .map(|seg| seg.split_whitespace().collect::<Vec<_>>())
        .filter(|tokens| !tokens.is_empty())
        .any(|tokens| segment_runs_build_or_test(&tokens, depth))
}

/// 只剥 `"` 与 `'`。引号字符用 `char::from(34)` / `char::from(39)` 构造、**不写字面量**：
/// 本仓 CI 的 lizard 会把 `'_'` 这类字符字面量当成 Rust **生命周期起始**（`'_` / `'a`），
/// 字符串扫描状态错乱后连带把后续函数边界算错（曾把本文件下一个函数算成 426 行）。
fn strip_shell_quotes(token: &str) -> &str {
    let double_quote = char::from(34_u8);
    let single_quote = char::from(39_u8);
    token.trim_matches(|c: char| c == double_quote || c == single_quote)
}

/// 形式上用字节判定（`b == 95`，`95` 是下划线 `_` 的 ASCII 码）而非 `'_'` 字面量：
/// 本仓 lizard 会把 `'_'` 当生命周期起始，进而错算本文件的函数边界。
///
/// `pub(crate)`：`completion_suppression` 的 argv 剥壳也用它——#1422 review 指出那里
/// 曾逐字重抄一份，正是本 issue 要收敛的「同一语义两套实现」。
pub(crate) fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once("=") else {
        return false;
    };
    !name.is_empty() && !name.starts_with("-") && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == 95)
}

/// 单条命令（已切段）是否在跑构建/测试。
fn segment_runs_build_or_test(tokens: &[&str], depth: usize) -> bool {
    let Some(index) = first_command_index(tokens) else {
        return false;
    };
    let Some(prog) = command_basename(tokens, index) else {
        return false;
    };
    let rest: Vec<&str> = tokens[index.saturating_add(1)..]
        .iter()
        .map(|t| strip_shell_quotes(t))
        .collect();
    program_runs_build_or_test(prog, &rest, depth)
}

/// 跳过 `VAR=val` 赋值、`cd <dir>`（占两个 token）与 `sudo` / `env` 等包装，返回程序名下标。
fn first_command_index(tokens: &[&str]) -> Option<usize> {
    let mut i = 0usize;
    while let Some(token) = tokens.get(i) {
        let bare = strip_shell_quotes(token);
        if is_env_assignment(bare) {
            i += 1;
        } else if bare == "cd" {
            i += 2;
        } else if matches!(bare, "sudo" | "env" | "time" | "command" | "exec") {
            i += 1;
        } else {
            return Some(i);
        }
    }
    None
}

/// 程序名取 basename：`/usr/bin/make` 与 `make` 同义。
fn command_basename<'a>(tokens: &'a [&'a str], index: usize) -> Option<&'a str> {
    let token = tokens.get(index)?;
    let base = strip_shell_quotes(token)
        .rsplit('/')
        .next()
        .unwrap_or_default();
    (!base.is_empty()).then_some(base)
}

/// 根据程序名 + 参数判定是否在跑构建/测试。
fn program_runs_build_or_test(prog: &str, rest: &[&str], depth: usize) -> bool {
    /// 自身即「构建/测试」的程序，不需要再看子命令。
    const STANDALONE: &[&str] = &[
        "make", "cmake", "ctest", "pytest", "ninja", "meson", "mvn", "gradle",
    ];
    if STANDALONE.contains(&prog) {
        return true;
    }
    if is_shell_wrapper(prog) {
        return shell_wrapped_command_runs_build(rest, depth);
    }
    runs_build_by_program_and_verb(prog, rest)
}

/// `bash` / `sh` 等：真实命令在 `-c` 的参数里。
fn is_shell_wrapper(prog: &str) -> bool {
    matches!(prog, "bash" | "sh" | "zsh" | "dash")
}

fn shell_wrapped_command_runs_build(rest: &[&str], depth: usize) -> bool {
    if depth >= MAX_SHELL_WRAP_DEPTH {
        return false;
    }
    let Some(pos) = rest.iter().position(|t| *t == "-c") else {
        return false;
    };
    let script = rest[pos.saturating_add(1)..].join(" ");
    command_line_looks_like_verify_inner(&script, depth.saturating_add(1))
}

fn runs_build_by_program_and_verb(prog: &str, rest: &[&str]) -> bool {
    /// `cargo <verb>` 里算验证的子命令。
    const CARGO_VERBS: &[&str] = &["test", "check", "clippy", "build", "nextest"];
    /// `npm/pnpm/yarn <verb>` 里算验证的子命令。
    const NODE_VERBS: &[&str] = &["test", "build"];
    // `python -m pytest` / `python3.12 -m pytest`：模块方式跑 pytest。
    if prog.starts_with("python") {
        return rest.windows(2).any(|w| w[0] == "-m" && w[1] == "pytest");
    }
    let verb = first_verb(rest);
    match prog {
        "cargo" => verb.is_some_and(|v| CARGO_VERBS.contains(&v)),
        "go" => verb == Some("test"),
        "npm" | "pnpm" | "yarn" => verb.is_some_and(|v| NODE_VERBS.contains(&v)),
        _ => false,
    }
}

/// 第一个「动词」：跳过 `+toolchain`（`cargo +nightly test`）与 `-` 开头的 flag。
fn first_verb<'a>(rest: &[&'a str]) -> Option<&'a str> {
    rest.iter()
        .copied()
        .find(|t| !t.is_empty() && !t.starts_with('+') && !t.starts_with('-'))
}

/// 输出文本里的构建 / 测试特征——**仅用于没有结构化 `invocation` 的旧消息**。
///
/// 去掉了 `make` / `cmake` 这两个短词：它们正是本函数最主要的误命中来源
/// （`make sure …`、文档正文里的 `cmake` 都算），而真正的 `make` / `cmake` 命令
/// 现在由 [`command_line_looks_like_verify`] 从命令行识别。
fn run_command_output_looks_like_verify(text_lower: &str) -> bool {
    const MARKERS: &[&str] = &[
        "cargo test",
        "cargo check",
        "cargo clippy",
        "cargo build",
        "pytest",
        "built target",
        "linking cxx executable",
        "test result: ok",
        "tests passed",
        "build succeeded",
    ];
    MARKERS.iter().any(|m| text_lower.contains(m))
}

/// 按 Unicode scalar 截断并压成单行，避免证据正文塞满上下文。
fn summarize_output(s: &str) -> String {
    let one_line: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let trimmed = one_line.trim();
    if trimmed.chars().count() <= DETAIL_MAX_CHARS {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(DETAIL_MAX_CHARS).collect();
    out.push('…');
    out
}

/// 单条工具消息 → 一条证据；无法归类的工具也记（供判定层看到「做过什么」）。
fn evidence_from_tool_message(
    fallback_name: &str,
    raw: &str,
    index: usize,
) -> Option<EvidenceItem> {
    let env = normalize_tool_message_content(raw);
    let (tool, output, ok, exit_code) = match env.as_ref() {
        Some(e) => (
            e.name.as_str(),
            e.output.as_str(),
            e.ok || e.exit_code == Some(0),
            e.exit_code,
        ),
        None => {
            // 无信封的历史行：只能靠启发式解析，强度必然是弱证据。
            let parsed = parse_tool_output(fallback_name, raw);
            (
                fallback_name,
                raw,
                parsed.ok || parsed.exit_code == Some(0),
                parsed.exit_code,
            )
        }
    };
    if tool.trim().is_empty() {
        return None;
    }
    let lower = format!("{output}\n{raw}").to_lowercase();
    // 命令是结构化字段：优先按它判定「这次算不算验证性执行」，输出文本只作旧消息的兜底。
    let invocation = env
        .as_ref()
        .and_then(|e| e.structured_payload.as_ref())
        .and_then(|v| v.get("invocation"))
        .and_then(serde_json::Value::as_str);
    let (kind, strength) = if is_dedicated_verify_tool(tool) {
        // 专用工具：有结构化退出码才算强证据。
        let strength = if exit_code.is_some() {
            EvidenceStrength::Strong
        } else {
            EvidenceStrength::Weak
        };
        (EvidenceKind::Verify, strength)
    } else if is_workspace_write_tool(tool) {
        (EvidenceKind::Write, EvidenceStrength::Strong)
    } else if tool == tool_names::RUN_COMMAND && run_command_is_verify(invocation, &lower) {
        (EvidenceKind::Verify, EvidenceStrength::Weak)
    } else {
        (EvidenceKind::Other, EvidenceStrength::Weak)
    };
    Some(EvidenceItem {
        id: format!("ev_{}", index + 1),
        kind,
        strength,
        tool: tool.to_string(),
        ok,
        exit_code,
        detail: summarize_output(output),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{msg, tool_env};

    fn user(text: &str) -> Message {
        msg("user", text)
    }

    #[test]
    fn assistant_claim_is_never_evidence() {
        // 只有 assistant 自述、没有任何工具消息 → 零证据。
        let messages = vec![
            user("把常量改成 42"),
            msg("assistant", "已完成修改，任务完成。"),
        ];
        assert!(EvidenceSet::collect(&messages).is_empty());
    }

    #[test]
    fn dedicated_cargo_test_is_strong_verify() {
        // 旧的 `generic_tool_is_build_or_test_success` 会因 tool != run_command 漏掉它。
        let messages = vec![
            user("跑通全部测试"),
            tool_env(
                tool_names::CARGO_TEST,
                "cargo test",
                "退出码：0\n标准输出：\ntest result: ok. 100 passed; 0 failed",
            ),
        ];
        let ev = EvidenceSet::collect(&messages);
        let item = &ev.items()[0];
        assert_eq!(item.kind, EvidenceKind::Verify);
        assert_eq!(item.strength, EvidenceStrength::Strong);
        assert_eq!(item.exit_code, Some(0));
        assert_eq!(item.id, "ev_1");
    }

    #[test]
    fn run_command_verify_is_only_weak() {
        let messages = vec![
            user("编译 hpcg"),
            tool_env(
                tool_names::RUN_COMMAND,
                "make",
                "命令：make\n退出码：0\n标准输出：\nBuilt target hpcg",
            ),
        ];
        let ev = EvidenceSet::collect(&messages);
        let item = &ev.items()[0];
        assert_eq!(item.kind, EvidenceKind::Verify);
        assert_eq!(
            item.strength,
            EvidenceStrength::Weak,
            "run_command 靠文本特征，只能是弱证据"
        );
    }

    #[test]
    fn write_tool_is_structured_write_evidence() {
        let messages = vec![
            user("改常量"),
            tool_env(tool_names::MODIFY_FILE, "修改 src/x.rs", "已修改文件: src/x.rs"),
        ];
        let ev = EvidenceSet::collect(&messages);
        let item = &ev.items()[0];
        assert_eq!(item.kind, EvidenceKind::Write, "写盘不是验证证据");
        assert_eq!(item.strength, EvidenceStrength::Strong);
    }

    #[test]
    fn failed_verify_is_not_success() {
        let messages = vec![
            user("跑测试"),
            tool_env(
                tool_names::CARGO_TEST,
                "cargo test",
                "退出码：101\n标准错误：\ntest result: FAILED. 3 passed; 1 failed",
            ),
        ];
        let ev = EvidenceSet::collect(&messages);
        let item = &ev.items()[0];
        assert_eq!(item.kind, EvidenceKind::Verify);
        assert!(!item.ok, "非零退出码不得算成功证据");
        assert_eq!(item.exit_code, Some(101));
    }

    /// #1401 #19：判定「这次跑的是不是构建/测试」看**命令行**，不猜输出文本。
    #[test]
    fn command_line_recognizes_build_and_test_programs() {
        for inv in [
            "cargo test",
            "cargo nextest run",
            "cargo check --all-targets",
            "cargo +nightly test",
            "cd build && make -j4",
            "cmake --build build",
            "/usr/bin/make -j8",
            "pytest -q",
            "python3 -m pytest -q",
            "python3.12 -m pytest",
            "go test ./...",
            "npm test",
            "ninja",
            "VAR=1 cargo test",
            "RUST_LOG=debug cargo test --locked",
            "sudo make install",
            "bash -c \"cargo test\"",
            "sh -c 'cargo test'",
            "bash -c \"cd build && make\"",
        ] {
            assert!(
                command_line_looks_like_verify(inv),
                "应判为验证性执行：{inv}"
            );
        }
    }

    #[test]
    fn command_line_rejects_non_build_commands() {
        for inv in [
            "git status",
            "ls -la",
            "cargo fmt",
            "cargo +nightly fmt",
            "cargo metadata",
            "go build ./...",
            "npm install",
            "makefile-editor",
            "echo make",
            "cat README.md",
            "python3 script.py",
            "bash -c \"git status\"",
        ] {
            assert!(
                !command_line_looks_like_verify(inv),
                "不应判为验证性执行：{inv}"
            );
        }
    }

    /// 旧实现的主要误命中来源：`make` / `cmake` 作为**普通词**出现在输出正文里。
    #[test]
    fn output_fallback_ignores_bare_words() {
        assert!(
            !run_command_output_looks_like_verify("please make sure the file exists"),
            "`make sure` 不是构建"
        );
        assert!(
            !run_command_output_looks_like_verify("see the cmake docs for details"),
            "正文提到 cmake 不是构建"
        );
        assert!(run_command_output_looks_like_verify("built target hpcg"));
        assert!(run_command_output_looks_like_verify(
            "test result: ok. 12 passed; 0 failed"
        ));
    }

    /// 有结构化命令时**只看命令**，输出文本不参与；无命令的旧消息才退回文本。
    #[test]
    fn invocation_takes_precedence_over_output_text() {
        assert!(
            !run_command_is_verify(Some("git status"), "please make sure the file exists"),
            "命令不是构建/测试时，输出里的 `make` 不得翻案"
        );
        assert!(run_command_is_verify(Some("make"), "whatever"));
        assert!(run_command_is_verify(None, "built target hpcg"));
        assert!(!run_command_is_verify(
            None,
            "please make sure the file exists"
        ));
    }

    /// #19 全链路：从 `crabmate_tool` 信封里读 `structured_payload.invocation`。
    ///
    /// `test_fixtures::tool_env` 不写 `structured_payload`，所以前面几条只能测到纯函数与文本兜底。
    /// 这条手搓信封，专门钉「提取链路」——将来 `structured_payload_for_tool` 改了形状，它会红。
    #[test]
    fn invocation_is_read_from_envelope_payload() {
        fn envelope(invocation: &str, output: &str) -> Message {
            msg(
                "tool",
                &serde_json::json!({
                    "crabmate_tool": {
                        "name": "run_command",
                        "summary": invocation,
                        "output": output,
                        "ok": true,
                        "exit_code": 0,
                        "structured_payload": {
                            "kind": "crabmate_structured_payload",
                            "tool": "run_command",
                            "version": 1,
                            "schema": "run_command_exit_v1",
                            "invocation": invocation,
                            "ok": true,
                            "exit_code": 0,
                            "stdout_nonempty": true,
                            "stderr_nonempty": false,
                        }
                    }
                })
                .to_string(),
            )
        }

        // 命令是 `cargo nextest`：输出文本里没有任何构建特征，只有读命令才能判对。
        let messages = vec![user("跑一下"), envelope("cargo nextest run", "Running tests…")];
        assert_eq!(
            EvidenceSet::collect(&messages).items()[0].kind,
            EvidenceKind::Verify,
            "应从 invocation 认出 cargo nextest"
        );

        // 命令是 `git status`：输出里出现 `make` 不得翻案。
        let messages = vec![
            user("看一眼"),
            envelope("git status", "please make sure the tree is clean"),
        ];
        assert_eq!(
            EvidenceSet::collect(&messages).items()[0].kind,
            EvidenceKind::Other,
            "命令不是构建/测试时，输出文本不得把证据翻成 Verify"
        );
    }

    #[test]
    fn ids_are_sequential_and_stable() {
        let messages = vec![
            user("分析"),
            tool_env(tool_names::LIST_TREE, "list tree: .", "list tree: ."),
            tool_env(tool_names::READ_FILE, "read file: a", "read file: a"),
        ];
        let ev = EvidenceSet::collect(&messages);
        let ids: Vec<&str> = ev.items().iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["ev_1", "ev_2"]);
    }

    #[test]
    fn detail_is_bounded_and_single_line() {
        let long = format!("行1\n{}", "x".repeat(1000));
        let messages = vec![
            user("x"),
            tool_env(tool_names::CARGO_TEST, "cargo test", &long),
        ];
        let ev = EvidenceSet::collect(&messages);
        let d = &ev.items()[0].detail;
        assert!(!d.contains('\n'), "证据正文压成单行");
        assert!(d.chars().count() <= DETAIL_MAX_CHARS + 1);
    }

    #[test]
    fn verify_tool_without_exit_code_degrades_to_weak() {
        // 无信封的历史行（有工具名、无 crabmate 信封）→ 解析不出退出码 → 强证据不成立。
        //
        // 必须给 `name`：没有工具名的 tool 消息在 `collect` 阶段就被丢弃（`tool.trim().is_empty()`），
        // 那样断言会作用在空集合上、恒真——原写法正是这个假通过。
        let messages = vec![
            user("跑测试"),
            Message {
                role: "tool".to_string(),
                content: Some("cargo test 完成了".to_string().into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some(tool_names::CARGO_TEST.to_string()),
                tool_call_id: Some("call_1".to_string()),
            },
        ];
        let ev = EvidenceSet::collect(&messages);
        let item = &ev.items()[0];
        assert_eq!(item.strength, EvidenceStrength::Weak);
        assert_eq!(item.exit_code, None, "无信封解析不出退出码");
    }
}

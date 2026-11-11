//! 上下文压缩的 characterization golden（#1347 S1）。
//!
//! **它守什么**：`compact_messages_to_token_budget` 的**判定**——什么时候触发、压哪一级、
//! 停在哪。S2 把「决策」抽成纯 policy（[`super::policy`]）后，本文件同时守两条：
//! ① 执行结果的结构字段不变；② policy 给出的**动作序列**（级数与顺序）与夹具一致。
//!
//! **它不守什么**：tiktoken 的精确水位。分词器 / 模型映射升级不该让这里变红，所以只断言
//! 结构字段（`token_triggered` / 删组数 / 剩余组数 / `compaction_reason` / 是否有降级），
//! 精确数字由 `context_compaction` 既有单测覆盖。
//!
//! 夹具：`fixtures/context_compaction_golden.json`（与 SSE golden 同族，单文件多 case）。

use serde::Deserialize;

use crate::agent::message_pipeline::conversation_turn_groups;
use crate::config::AgentConfig;
use crate::types::Message;

use super::policy::{CompactionProbe, plan_compaction};
use super::{ContextTokenBudget, compact_messages_to_token_budget, estimate_final_request_tokens};

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    config: ConfigSpec,
    messages: Vec<MsgSpec>,
    #[serde(default)]
    summarized: bool,
    #[serde(default)]
    min_tail: usize,
    expect: Expect,
}

#[derive(Deserialize)]
struct ConfigSpec {
    llm_context_tokens: u32,
    max_tokens: u32,
    #[serde(default)]
    trigger_percent: Option<u32>,
    #[serde(default)]
    target_percent: Option<u32>,
    #[serde(default)]
    safety_margin: Option<u32>,
}

/// 消息构造指令：只描述「形状」，正文按行数铺开成高熵文本（避免 BPE 把重复字符压成超长 token）。
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum MsgSpec {
    System {
        text: String,
    },
    /// 摘要注入（`crabmate_context_summary`）：让「摘要 + 尾部」这一形态可复现。
    Summary {
        text: String,
    },
    Turn {
        #[serde(default)]
        user_lines: usize,
        #[serde(default)]
        tool_lines: usize,
        #[serde(default)]
        assistant: String,
    },
}

#[derive(Deserialize)]
struct Expect {
    token_triggered: bool,
    removed_turn_groups: usize,
    groups_left: usize,
    reason: String,
    /// 旧组 `tool` 正文被二次压缩的条数下限。
    #[serde(default)]
    degraded_min: usize,
    /// 决策**动作序列**（级数与顺序），由 `policy::ContextAction::kind` 命名。
    ///
    /// 与 `removed_turn_groups` 互补：动作是「打算压哪几级」，删组数是「实际动了多少」。
    /// 例如 `old_tool_body_degrade_alone_suffices` 两个动作都在，却一组都没删。
    #[serde(default)]
    actions: Vec<String>,
}

fn load_fixture() -> Fixture {
    serde_json::from_str(include_str!("../../../fixtures/context_compaction_golden.json"))
        .expect("parse fixtures/context_compaction_golden.json")
}

fn bulky_text(tag: &str, lines: usize) -> String {
    (0..lines)
        .map(|i| format!("第{i}行 {tag} 内容 value_{i}\n"))
        .collect()
}

fn config_of(spec: &ConfigSpec) -> AgentConfig {
    let mut cfg = crate::config::load_config(None).expect("embedded config");
    cfg.llm_sampling.llm_context_tokens = spec.llm_context_tokens;
    cfg.llm_sampling.max_tokens = spec.max_tokens;
    if let Some(p) = spec.trigger_percent {
        cfg.context_pipeline.context_token_trigger_percent = p;
    }
    if let Some(p) = spec.target_percent {
        cfg.context_pipeline.context_token_target_percent = p;
    }
    if let Some(m) = spec.safety_margin {
        cfg.context_pipeline.context_token_safety_margin_tokens = m;
    }
    cfg
}

fn build_messages(specs: &[MsgSpec]) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    for (idx, spec) in specs.iter().enumerate() {
        match spec {
            MsgSpec::System { text } => out.push(Message::system_only(text.clone())),
            MsgSpec::Summary { text } => out.push(Message::user_context_summary_injection(text.clone())),
            MsgSpec::Turn {
                user_lines,
                tool_lines,
                assistant,
            } => {
                out.push(Message::user_only(format!(
                    "turn-{idx} {}",
                    bulky_text("u", *user_lines)
                )));
                if *tool_lines > 0 {
                    out.push(Message::assistant_only("calling tool"));
                    out.push(Message::tool_result_named(
                        format!("call-{idx}"),
                        "read_file".to_string(),
                        bulky_text("t", *tool_lines),
                    ));
                }
                out.push(Message::assistant_only(assistant.clone()));
            }
        }
    }
    out
}

/// 一次运行的结构化结果（用于断言与「同输入同动作」比较）。
#[derive(Debug, PartialEq, Eq)]
struct Shape {
    token_triggered: bool,
    /// policy 给出的触发判定：必须与报告的 `token_triggered` 同源（报告不再自己算一遍）。
    plan_token_triggered: bool,
    /// policy 给出的动作序列（级数与顺序）。
    actions: Vec<&'static str>,
    removed_turn_groups: usize,
    groups_left: usize,
    reason: &'static str,
    degraded: usize,
}

fn run_case(case: &Case) -> Shape {
    let cfg = config_of(&case.config);
    let mut messages = build_messages(&case.messages);
    let before = estimate_final_request_tokens(&cfg, &messages, &[], None);
    // 摘要还没发生，两个水位相同——与 `compact_messages_to_token_budget` 内部的取法一致。
    let plan = plan_compaction(&CompactionProbe {
        budget: ContextTokenBudget::from_config(&cfg),
        before,
        current: before,
        min_tail_messages: case.min_tail,
    });
    let report = compact_messages_to_token_budget(
        &cfg,
        &mut messages,
        &[],
        None,
        before,
        case.summarized,
        case.min_tail,
    );
    Shape {
        token_triggered: report.token_triggered,
        plan_token_triggered: plan.token_triggered,
        actions: plan.kinds(),
        removed_turn_groups: report.removed_turn_groups,
        groups_left: conversation_turn_groups(&messages).len(),
        reason: report.compaction_reason(),
        degraded: report.compressed_old_tool_messages,
    }
}

#[test]
fn golden_context_compaction_matches_fixture() {
    let fixture = load_fixture();
    assert!(!fixture.cases.is_empty(), "golden 不得为空");
    for case in &fixture.cases {
        let got = run_case(case);
        let expect = &case.expect;
        assert_eq!(
            got.token_triggered, expect.token_triggered,
            "case={} token_triggered 不符: {got:?}",
            case.name
        );
        assert_eq!(
            got.plan_token_triggered, got.token_triggered,
            "case={} 报告的 token_triggered 必须来自 policy（不许再各算一遍）: {got:?}",
            case.name
        );
        let expected_actions: Vec<&str> = expect.actions.iter().map(String::as_str).collect();
        assert_eq!(
            got.actions, expected_actions,
            "case={} 动作序列（级数与顺序）不符: {got:?}",
            case.name
        );
        assert_eq!(
            got.removed_turn_groups, expect.removed_turn_groups,
            "case={} 删组数不符: {got:?}",
            case.name
        );
        assert_eq!(
            got.groups_left, expect.groups_left,
            "case={} 剩余组数不符: {got:?}",
            case.name
        );
        assert_eq!(
            got.reason, expect.reason,
            "case={} compaction_reason 不符: {got:?}",
            case.name
        );
        assert!(
            got.degraded >= expect.degraded_min,
            "case={} 期望至少 {} 条旧 tool 正文被降级，实际 {}: {got:?}",
            case.name,
            expect.degraded_min,
            got.degraded
        );
    }
}

/// 同输入 → 同动作：S2 抽 policy 时，这条比逐字段断言更能守住「没悄悄改判定」。
#[test]
fn golden_context_compaction_is_deterministic() {
    let fixture = load_fixture();
    for case in &fixture.cases {
        assert_eq!(
            run_case(case),
            run_case(case),
            "case={} 同输入两次运行结果不一致（删组/降级必须确定性）",
            case.name
        );
    }
}

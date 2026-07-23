//! e2e 场景定义、指标收集与统一 runner（CLI 子命令 `crabmate e2e` 与集成测试共用）。
//!
//! [`TestScenario`] 描述一个测试场景（用户消息 + 可选工作区文件），
//! 支持 serde 反序列化，场景可从外部 JSON/YAML 文件加载。
//! [`TestRunMetrics`] 收集执行指标，[`run_scenario`] 封装完整的执行流程。
//!
//! # 使用方式（测试）
//!
//! ```ignore
//! use crabmate::e2e_scenario::{TestScenario, run_scenario};
//!
//! let metrics = run_scenario(&TestScenario {
//!     name: "my_test".into(),
//!     user_message: "你好".into(),
//!     workspace_files: vec![],
//!     expected_output_contains: vec![],
//!     expected_tool_used: None,
//! }, &e2e_cfg).await;
//! assert!(metrics.success, "{} should succeed", metrics.scenario);
//! ```
//!
//! # CLI 外部场景文件
//!
//! `crabmate e2e --scenarios-file scenarios.json` 支持 JSON 格式：
//!
//! ```json
//! [
//!   {
//!     "name": "custom_test",
//!     "user_message": "Run the Python script",
//!     "workspace_files": [["hello.py", "print('hello')"]],
//!     "expected_output_contains": ["hello"],
//!     "expected_tool_used": "run_command"
//!   }
//! ]
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crabmate_llm::{E2eMode, build_e2e_backend};

use crate::config::load_config;
use crate::context_bootstrap::conversation_turn_bootstrap::compose_new_conversation_messages;
use crate::context_bootstrap::prompt_compose::{
    FirstSystemComposeOpts, RoleSystemResolution, compose_first_system_for_turn,
};
use crate::tool_stats::ToolOutcomeRecorder;
use crate::{
    AgentConfig, AgentTurnLlmOverrides, AgentTurnTransport, ChatCompletionsBackend,
    LlmSeedOverride, Message, PlannerExecutorMode, ProcessHandles, RunAgentTurnParams,
    RunAgentTurnSharedInputs, build_tools, run_agent_turn,
};

/// LLM-as-Judge 评分配置。
#[derive(Debug, Clone, Default)]
pub struct JudgeConfig {
    /// 是否启用评分。
    pub enabled: bool,
    /// 评分模型（默认使用配置中的主模型）。
    pub model: Option<String>,
    /// 评分 API 地址（默认使用配置中的 api_base）。
    pub api_base: Option<String>,
}

/// LLM-as-Judge 评分结果。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JudgeResult {
    /// 评分 1-5，5 为最优。
    pub score: u8,
    /// 评分理由。
    pub rationale: String,
    /// 失败方面。
    #[serde(default)]
    pub failed_aspects: Vec<String>,
}

/// e2e runner 运行时配置。
pub struct E2eRunConfig {
    /// LLM API 密钥。
    pub api_key: String,
    /// artifact 输出根目录（默认 `.crabmate/e2e_artifacts`）。
    pub artifacts_root: PathBuf,
    /// 录制数据目录（默认 `tests/fixtures/llm_recordings`，CLI 可指定）。
    pub recordings_dir: PathBuf,
    /// e2e 模式（`E2eMode::Real` / `Record` / `Replay`）。
    pub mode: E2eMode,
    /// LLM-as-Judge 评分配置。
    pub judge_config: JudgeConfig,
}

impl Default for E2eRunConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            artifacts_root: PathBuf::from(".crabmate/e2e_artifacts"),
            recordings_dir: PathBuf::from("tests/fixtures/llm_recordings"),
            mode: E2eMode::Real,
            judge_config: JudgeConfig::default(),
        }
    }
}

/// 通用 e2e 测试场景定义。
///
/// 每个场景描述一个完整的测试任务：用户消息 + 可选工作区文件。
/// 测试框架会自动创建临时目录、写入文件、执行 agent turn、收集指标。
///
/// 支持 serde 反序列化，可从外部 JSON/YAML 文件加载。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestScenario {
    /// 场景名称（用于录制/artifact 目录路径）。
    pub name: String,
    /// 发送给 agent 的用户消息。
    pub user_message: String,
    /// 需要在临时工作区预创建的文件（文件名 → 内容）。
    #[serde(default)]
    pub workspace_files: Vec<(String, String)>,
    /// 期望最终回复包含的关键词（可选，多项时任意一项匹配即可）。
    #[serde(default)]
    pub expected_output_contains: Vec<String>,
    /// 期望 agent 使用的工具名称（可选）。
    pub expected_tool_used: Option<String>,
}

/// 单次 e2e 测试的结构化运行指标。
///
/// 每次 [`run_scenario`] 执行后生成，用于自动化分析、报告生成和多轮对比。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestRunMetrics {
    /// 场景名称。
    pub scenario: String,
    /// 是否整体成功（`run_agent_turn` 无 panic/error）。
    pub success: bool,
    /// 执行耗时（毫秒）。
    pub duration_ms: u64,
    /// LLM 调用轮数（assistant 消息数反映了 LLM 的交互轮数）。
    pub llm_rounds: usize,
    /// 工具调用总数。
    pub tool_call_count: usize,
    /// 工具调用报错数。
    pub tool_errors: usize,
    /// 工具报错后是否自我恢复（后续仍有非工具消息的 assistant 回复）。
    pub tool_error_recovered: bool,
    /// 实际使用的工具名称列表（去重）。
    pub tool_names: Vec<String>,
    /// 总消息数。
    pub total_messages: usize,
    /// 末条消息的角色。
    pub last_role: String,
    /// 末条消息内容的前 300 字符（方便快速审查）。
    pub final_output_preview: String,
    /// 最终回复是否包含期望关键词。
    pub expected_output_matched: bool,
    /// 是否使用了期望的工具。
    pub expected_tool_matched: bool,
    /// 错误消息（如有）。
    pub error_message: Option<String>,
    /// 制作 data （ISO 8601）。
    pub timestamp: String,
    /// LLM-as-Judge 评分（启用时才有）。
    pub judge_score: Option<u8>,
    /// 评分理由。
    pub judge_rationale: Option<String>,
    /// 失败方面。
    pub judge_failed_aspects: Option<Vec<String>>,
}

/// 运行单个 e2e 场景，返回结构化指标。
///
/// 该函数封装了完整的执行流程：
/// 1. 创建临时工作区，写入 `workspace_files`
/// 2. 使用生产路径构建 system prompt + user message
/// 3. 调用 `run_agent_turn` 执行 agent 回合
/// 4. 提取结构化指标，导出消息记录到 artifact 目录
/// 5. 返回 [`TestRunMetrics`]
pub async fn run_scenario(scenario: &TestScenario, e2e_cfg: &E2eRunConfig) -> TestRunMetrics {
    let start = Instant::now();
    let scenario_name = scenario.name.as_str();
    let artifacts_dir = e2e_cfg.artifacts_root.join(scenario_name);
    let _ = std::fs::create_dir_all(&artifacts_dir);

    // 1. 创建临时工作区，写入文件
    let (work_dir, _tmp_holder) = setup_workspace(scenario);

    // 2. 构建配置和 messages
    let cfg = cfg_single_agent();
    let mut messages = build_turn_messages(&cfg, &scenario.user_message);
    let workspace_is_set = !scenario.workspace_files.is_empty();

    // 3. 执行 agent turn
    let turn_result = run_single_agent_turn(SingleAgentTurnCfg {
        test_name: scenario_name,
        cfg: &cfg,
        messages: &mut messages,
        work_dir: &work_dir,
        workspace_is_set,
        api_key: &e2e_cfg.api_key,
        recordings_dir: &e2e_cfg.recordings_dir,
        mode: e2e_cfg.mode,
    })
    .await;

    let elapsed = start.elapsed();
    let duration_ms = elapsed.as_millis() as u64;

    // 4. 提取指标
    let success = turn_result.is_ok();
    let error_message = turn_result.as_ref().err().map(|e| e.to_string());

    let mut metrics = extract_metrics(scenario, &messages, success, duration_ms, error_message);

    // 5. LLM-as-Judge 评分
    if e2e_cfg.judge_config.enabled
        && let Some(judge) =
            judge_scenario(scenario, &messages, &e2e_cfg.api_key, &e2e_cfg.judge_config).await
    {
        metrics.judge_score = Some(judge.score);
        metrics.judge_rationale = Some(judge.rationale);
        if !judge.failed_aspects.is_empty() {
            metrics.judge_failed_aspects = Some(judge.failed_aspects);
        }
    }

    // 6. 导出报告
    export_artifacts(&metrics, &messages, &artifacts_dir);

    metrics
}

/// CLI 入口：运行预设场景或从外部文件加载的场景，生成报告。
///
/// `scenarios_file` 为 `Some(path)` 时从文件加载场景（JSON/YAML），否则使用预设场景。
/// 供 `cli_run.rs` 中的 `crabmate e2e` 子命令调用。
pub async fn run_e2e_cli(
    e2e_cfg: &E2eRunConfig,
    scenarios_file: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let scenarios = match scenarios_file {
        Some(path) => load_scenarios_file(path)?,
        None => preset_scenarios(),
    };

    let total = scenarios.len();
    let mut results: Vec<TestRunMetrics> = Vec::with_capacity(total);

    for scenario in &scenarios {
        eprintln!(">>> 执行场景: {}", scenario.name);
        let metrics = run_scenario(scenario, e2e_cfg).await;
        results.push(metrics);
    }

    // Markdown 报告
    let report_path = e2e_cfg.artifacts_root.join("scenario_report.md");
    generate_report(&results, &report_path);
    eprintln!("\nE2E 测试报告: {}", report_path.display());

    // JSON 报告
    let json_path = e2e_cfg.artifacts_root.join("scenario_report.json");
    if let Ok(json) = serde_json::to_string_pretty(&results) {
        std::fs::write(&json_path, &json)?;
    }

    // 检查失败场景
    let failures: Vec<_> = results.iter().filter(|r| !r.success).collect();
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("  失败: {} — {:?}", f.scenario, f.error_message);
        }
        return Err(format!("{} / {} scenarios failed", failures.len(), total).into());
    }

    eprintln!("所有 {} 个场景通过。", total);
    Ok(())
}

/// 预设场景列表（与集成测试 `e2e_all_scenarios` 一致）。
pub fn preset_scenarios() -> Vec<TestScenario> {
    vec![
        TestScenario {
            name: "orch_single_agent_smoke".to_string(),
            user_message: "你好，用一句话介绍自己。".to_string(),
            workspace_files: vec![],
            expected_output_contains: vec![],
            expected_tool_used: None,
        },
        TestScenario {
            name: "orch_single_agent_tool".to_string(),
            user_message: "请调用 get_current_time 工具查询当前时间，然后用一句话总结。"
                .to_string(),
            workspace_files: vec![],
            expected_output_contains: vec![],
            expected_tool_used: Some("get_current_time".to_string()),
        },
        TestScenario {
            name: "orch_cpp_cmake".to_string(),
            user_message: "请查看工作区中的 C++ 项目，编译并运行它，然后告诉我输出结果。"
                .to_string(),
            workspace_files: vec![
                (
                    "main.cpp".to_string(),
                    r#"#include <iostream>
int main() {
    std::cout << "Hello from C++!" << std::endl;
    return 0;
}
"#
                    .to_string(),
                ),
                (
                    "CMakeLists.txt".to_string(),
                    r#"cmake_minimum_required(VERSION 3.10)
project(cpp_e2e_test)
add_executable(cpp_e2e_test main.cpp)
"#
                    .to_string(),
                ),
            ],
            expected_output_contains: vec!["Hello from C++".to_string(), "编译成功".to_string()],
            expected_tool_used: Some("run_command".to_string()),
        },
    ]
}

/// 从外部 JSON/YAML 文件加载场景列表。
///
/// 格式见模块文档。支持 `.json` 和 `.yaml`/`.yml` 扩展名。
pub fn load_scenarios_file(path: &Path) -> Result<Vec<TestScenario>, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let scenarios = match ext {
        "yaml" | "yml" => serde_yaml::from_str(&content)?,
        _ => serde_json::from_str(&content)?,
    };
    Ok(scenarios)
}

/// 生成 e2e 测试报告（Markdown），包含所有场景的指标对比。
pub fn generate_report(results: &[TestRunMetrics], report_path: &Path) {
    let total = results.len();
    let successes = results.iter().filter(|r| r.success).count();

    let mut body = String::new();
    body.push_str("# E2E 测试报告\n\n");
    body.push_str(&format!(
        "- **执行时间**: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));
    body.push_str(&format!("- **场景总数**: {total}\n"));
    body.push_str(&format!("- **成功**: {successes}\n"));
    body.push_str(&format!("- **失败**: {}\n\n", total - successes));

    body.push_str("| 场景 | 成功 | 耗时(ms) | LLM轮数 | 工具调用 | 工具报错 | 恢复 | 末条角色 | 期望输出匹配 | Judge评分 |\n");
    body.push_str("|------|------|----------|---------|----------|----------|------|----------|--------------|-----------|\n");

    for r in results {
        body.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.scenario,
            if r.success { "✅" } else { "❌" },
            r.duration_ms,
            r.llm_rounds,
            r.tool_call_count,
            r.tool_errors,
            if r.tool_error_recovered { "✅" } else { "❌" },
            r.last_role,
            if r.expected_output_matched {
                "✅"
            } else {
                "❌"
            },
            r.judge_score
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".to_string()),
        ));
    }

    body.push_str("\n---\n\n### 各场景详情\n\n");

    for r in results {
        body.push_str(&format!(
            "#### {}\n- 耗时: {}ms\n- LLM轮数: {}\n- 工具: {:?}\n- Judge: {}\n- 最终回复预览: `{}...`\n- 错误: {:?}\n\n",
            r.scenario,
            r.duration_ms,
            r.llm_rounds,
            r.tool_names,
            r.judge_score.map(|s| format!("{}/5", s)).unwrap_or_else(|| "未启用".to_string()),
            r.final_output_preview.chars().take(200).collect::<String>(),
            r.error_message,
        ));
    }

    let _ = std::fs::create_dir_all(report_path.parent().unwrap_or(Path::new(".")));
    let _ = std::fs::write(report_path, &body);
}

// ---------------------------------------------------------------------------
// 内部辅助函数
// ---------------------------------------------------------------------------

/// 为场景创建临时工作区并写入文件。
fn setup_workspace(scenario: &TestScenario) -> (PathBuf, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("创建临时目录失败");
    let work_dir = tmp.path().to_path_buf();

    for (file_name, content) in &scenario.workspace_files {
        let full_path = work_dir.join(file_name);
        if let Some(parent) = full_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&full_path, content).unwrap_or_else(|e| {
            panic!("写入工作区文件 {file_name} 失败: {e}");
        });
    }

    (work_dir, tmp)
}

/// 构造 SingleAgent 配置（禁用 intent routing）。
fn cfg_single_agent() -> Arc<AgentConfig> {
    let mut cfg = load_config(None).expect("embedded default config must load");
    cfg.per_plan_policy.planner_executor_mode = PlannerExecutorMode::SingleAgent;
    // intent_at_turn_start_enabled / intent_l2_enabled 已从 config 删除（L1）；硬编码 false。
    Arc::new(cfg)
}

/// 使用生产路径构建首轮 messages（system prompt + user message）。
fn build_turn_messages(cfg: &AgentConfig, user_text: &str) -> Vec<Message> {
    let recorder = Arc::new(ToolOutcomeRecorder::new());
    let system = compose_first_system_for_turn(
        cfg,
        &recorder,
        FirstSystemComposeOpts {
            agent_role: None,
            user_msg_for_skills: None,
            skills_base_dir: None,
            role_resolution: RoleSystemResolution::Strict,
        },
    )
    .expect("compose_first_system_for_turn 应成功");

    compose_new_conversation_messages(&system, None, Some(Message::user_only(user_text)))
}

/// 单次 agent turn 执行的内部参数（减少 `run_single_agent_turn` 形参个数）。
struct SingleAgentTurnCfg<'a> {
    test_name: &'a str,
    cfg: &'a Arc<AgentConfig>,
    messages: &'a mut Vec<Message>,
    work_dir: &'a Path,
    workspace_is_set: bool,
    api_key: &'a str,
    recordings_dir: &'a Path,
    mode: E2eMode,
}

/// 构造 e2e 后端并注入 `RunAgentTurnParams`，执行单轮 agent turn。
async fn run_single_agent_turn(
    args: SingleAgentTurnCfg<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let backend_box = build_e2e_backend(
        args.mode,
        Box::new(crate::OpenAiCompatBackend),
        args.recordings_dir,
        args.test_name,
    )?;
    let backend_ref: &'static (dyn ChatCompletionsBackend + 'static) = Box::leak(backend_box);

    let client = reqwest::Client::new();
    let tools = build_tools();

    let params = RunAgentTurnParams {
        shared: RunAgentTurnSharedInputs {
            client: &client,
            api_key: args.api_key,
            cfg: args.cfg,
            tools: tools.as_slice(),
        },
        messages: args.messages,
        effective_working_dir: args.work_dir,
        workspace_is_set: args.workspace_is_set,
        transport: AgentTurnTransport {
            out: None,
            render_to_terminal: false,
            no_stream: true,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            cli_tool_ctx: None,
            plain_terminal_stream: false,
            tui_llm_stream_scratch: None,
            tool_running_hook: None,
            clarification_questionnaire_hook: None,
            sse_control_mirror: None,
            llm_backend: Some(backend_ref),
            trace_sink: None,
        },
        llm: AgentTurnLlmOverrides {
            temperature_override: None,
            model_override: None,
            use_executor_model: false,
            executor_model_override: None,
            executor_api_base: None,
            executor_api_key: None,
            seed_override: LlmSeedOverride::default(),
        },
        long_term_memory: None,
        long_term_memory_scope_id: None,
        read_file_turn_cache: None,
        turn_allowed_tool_names: None,
        tracing_chat_turn: None,
        request_audit: None,
        process_handles: ProcessHandles::default_arc_process_handles(),
    };

    let result = run_agent_turn(params).await;
    result?;
    Ok(())
}

/// 从 messages 中提取结构化指标。
fn extract_metrics(
    scenario: &TestScenario,
    messages: &[Message],
    success: bool,
    duration_ms: u64,
    error_message: Option<String>,
) -> TestRunMetrics {
    let now = chrono::Local::now();

    let total_messages = messages.len();
    let llm_rounds = messages.iter().filter(|m| m.role == "assistant").count();
    let last = messages.last();
    let last_role = last.map(|m| m.role.clone()).unwrap_or_default();

    // 统计工具调用
    let tool_calls: Vec<&crate::ToolCall> = messages
        .iter()
        .filter(|m| m.role == "assistant")
        .filter_map(|m| m.tool_calls.as_ref())
        .flat_map(|calls| calls.iter())
        .collect();

    let tool_call_count = tool_calls.len();
    let mut tool_names: Vec<String> = tool_calls
        .iter()
        .map(|tc| tc.function.name.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    tool_names.sort();

    // 统计工具报错 — tool 消息 content 包含 error/failed/panic 关键词
    let tool_errors = messages
        .iter()
        .filter(|m| m.role == "tool")
        .filter(|m| {
            if let Some(text) = crate::message_content_as_str(&m.content) {
                let lower = text.to_lowercase();
                lower.contains("error") || lower.contains("failed") || lower.contains("panic")
            } else {
                false
            }
        })
        .count();

    // 工具报错后是否恢复：报错后至少有一条 assistant 消息不含新 tool_calls
    let tool_error_recovered = if tool_errors > 0 {
        messages
            .iter()
            .rev()
            .take_while(|m| m.role != "tool")
            .any(|m| m.role == "assistant" && m.tool_calls.is_none())
    } else {
        true
    };

    // 最终回复预览
    let final_output_preview = last
        .and_then(|m| crate::message_content_as_str(&m.content))
        .unwrap_or("")
        .chars()
        .take(300)
        .collect::<String>();

    // 期望关键词匹配
    let expected_output_matched = if scenario.expected_output_contains.is_empty() {
        true
    } else {
        let body = last
            .and_then(|m| crate::message_content_as_str(&m.content))
            .unwrap_or("")
            .to_lowercase();
        scenario
            .expected_output_contains
            .iter()
            .any(|kw| body.contains(&kw.to_lowercase()))
    };

    // 期望工具匹配
    let expected_tool_matched = match scenario.expected_tool_used.as_deref() {
        Some(expected) => tool_names.iter().any(|n| n == expected),
        None => true,
    };

    TestRunMetrics {
        scenario: scenario.name.clone(),
        success,
        duration_ms,
        llm_rounds,
        tool_call_count,
        tool_errors,
        tool_error_recovered,
        tool_names,
        total_messages,
        last_role,
        final_output_preview,
        expected_output_matched,
        expected_tool_matched,
        error_message,
        timestamp: now.format("%Y-%m-%dT%H:%M:%S%z").to_string(),
        judge_score: None,
        judge_rationale: None,
        judge_failed_aspects: None,
    }
}

/// 导出消息记录和指标到 artifact 目录。
fn export_artifacts(metrics: &TestRunMetrics, messages: &[Message], artifacts_dir: &Path) {
    // JSON 指标报告
    if let Ok(json) = serde_json::to_string_pretty(metrics) {
        let _ = std::fs::write(artifacts_dir.join("metrics.json"), &json);
    }

    // Markdown 消息记录
    let md = dump_messages_markdown(messages, &metrics.scenario);
    let _ = std::fs::write(artifacts_dir.join("messages_final.md"), &md);

    // JSON 消息记录
    if let Ok(json) = serde_json::to_string_pretty(messages) {
        let _ = std::fs::write(artifacts_dir.join("messages_final.json"), &json);
    }

    // 简短摘要
    let summary = format!(
        r#"# {scenario}

- **成功**: {success}
- **耗时**: {duration_ms} ms
- **LLM 轮数**: {llm_rounds}
- **工具调用**: {tool_call_count}
- **工具报错**: {tool_errors}
- **末条角色**: {last_role}
- **期望输出匹配**: {output_matched}
- **期望工具匹配**: {tool_matched}
- **Judge 评分**: {judge_score}
- **Judge 理由**: {judge_rationale}
"#,
        scenario = metrics.scenario,
        success = metrics.success,
        duration_ms = metrics.duration_ms,
        llm_rounds = metrics.llm_rounds,
        tool_call_count = metrics.tool_call_count,
        tool_errors = metrics.tool_errors,
        last_role = metrics.last_role,
        output_matched = metrics.expected_output_matched,
        tool_matched = metrics.expected_tool_matched,
        judge_score = metrics
            .judge_score
            .map(|s| s.to_string())
            .unwrap_or_else(|| "未启用".to_string()),
        judge_rationale = metrics.judge_rationale.as_deref().unwrap_or("未启用"),
    );
    let _ = std::fs::write(artifacts_dir.join("summary.md"), &summary);
}

/// 将 messages 导出为可读的 Markdown 格式。
pub fn dump_messages_markdown(messages: &[Message], test_name: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("# E2E 测试消息记录: {test_name}\n\n"));
    out.push_str(&format!("- **总消息数**: {}\n", messages.len()));
    out.push_str(&format!(
        "- **时间**: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));
    out.push('\n');
    out.push_str("---\n\n");

    for (i, msg) in messages.iter().enumerate() {
        let role_desc = match msg.role.as_str() {
            "system" => "🧠 **System**".to_string(),
            "user" => "👤 **User**".to_string(),
            "assistant" => "🤖 **Assistant**".to_string(),
            "tool" => "🛠️ **Tool**".to_string(),
            r => format!("**{r}**"),
        };

        out.push_str(&format!("## 消息 {i}: {role_desc}\n\n"));

        // reasoning_content
        if let Some(ref r) = msg.reasoning_content
            && !r.is_empty()
        {
            out.push_str("### 推理过程\n\n```\n");
            out.push_str(r);
            out.push_str("\n```\n\n");
        }

        // content
        if let Some(text) = crate::message_content_as_str(&msg.content)
            && !text.is_empty()
        {
            out.push_str("### 内容\n\n```\n");
            out.push_str(text);
            out.push_str("\n```\n\n");
        }

        // tool_calls
        if let Some(ref calls) = msg.tool_calls {
            for tc in calls {
                out.push_str(&format!("### 🔧 工具调用: `{}`\n\n", tc.function.name));
                out.push_str(&format!("- **id**: `{}`\n", tc.id));
                out.push_str(&format!(
                    "- **参数**:\n\n```json\n{}\n```\n\n",
                    tc.function.arguments
                ));
            }
        }

        // tool result metadata
        if msg.role == "tool" {
            if let Some(ref name) = msg.name {
                out.push_str(&format!("- **工具**: `{name}`\n"));
            }
            if let Some(ref id) = msg.tool_call_id {
                out.push_str(&format!("- **tool_call_id**: `{id}`\n"));
            }
            out.push('\n');
        }

        out.push_str("---\n\n");
    }

    out
}

/// 用 LLM 对场景完成质量进行评分（LLM-as-Judge）。
///
/// 向配置的 LLM 端点发送单次 chat/completions 请求，要求模型以 JSON 格式
/// 对 assistant 的回复进行 1-5 评分并给出理由。
async fn judge_scenario(
    scenario: &TestScenario,
    messages: &[Message],
    api_key: &str,
    judge_cfg: &JudgeConfig,
) -> Option<JudgeResult> {
    if !judge_cfg.enabled || api_key.is_empty() {
        return None;
    }

    // 构建评分 prompt
    let last_assistant = messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && m.tool_calls.is_none());

    let final_output = last_assistant
        .and_then(|m| crate::message_content_as_str(&m.content))
        .unwrap_or("");

    let tool_trace: Vec<String> = messages
        .iter()
        .filter(|m| m.role == "assistant")
        .filter_map(|m| m.tool_calls.as_ref())
        .flat_map(|calls| calls.iter())
        .map(|tc| format!("  - {} (args: {})", tc.function.name, tc.function.arguments))
        .collect();

    let system_prompt = "You are an evaluator that rates AI assistant responses. \
        Score 1-5 based on: task completion, correctness, tool usage efficiency. \
        Respond in JSON only: {\"score\": <1-5>, \"rationale\": \"...\", \"failed_aspects\": [\"...\"]}";

    let user_prompt = format!(
        r#"## Task
{}

## Assistant's Final Output
{}

## Tool Calls Made
{}

## Expected Output Keywords
{}

## Expected Tool
{}
"#,
        scenario.user_message,
        final_output,
        if tool_trace.is_empty() {
            "(none)".to_string()
        } else {
            tool_trace.join("\n")
        },
        if scenario.expected_output_contains.is_empty() {
            "(none)".to_string()
        } else {
            scenario.expected_output_contains.join(", ")
        },
        scenario.expected_tool_used.as_deref().unwrap_or("(none)"),
    );

    let cfg = load_config(None).ok()?;
    let api_base = judge_cfg.api_base.as_deref().unwrap_or(&cfg.llm.api_base);
    let model = judge_cfg.model.as_deref().unwrap_or(&cfg.llm.model);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .ok()?;

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt}
        ],
        "temperature": 0.0,
        "max_tokens": 512,
    });

    let resp = client
        .post(format!("{api_base}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .ok()?;

    let resp_json: serde_json::Value = resp.json().await.ok()?;
    let content = resp_json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("{}");

    // 尝试从 JSON 中提取评分（模型可能返回 markdown code block）
    let cleaned = content
        .trim()
        .strip_prefix("```json")
        .or_else(|| content.trim().strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(content.trim());

    let judge: JudgeResult = serde_json::from_str(cleaned).ok()?;
    Some(judge)
}

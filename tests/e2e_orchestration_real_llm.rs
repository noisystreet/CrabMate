//! 编排级真实 LLM e2e 测试（Layer 2）。
//!
//! 覆盖 **SingleAgent** 编排模式，使用生产路径构建 system prompt，
//! 通过 [`crabmate::e2e_scenario`] 的 [`run_scenario`] 统一执行和指标收集。
//!
//! # 场景定义
//!
//! 每个场景由 [`TestScenario`] 描述，添加新场景只需在 `e2e_all_scenarios()`
//! 的 `scenarios` 列表中添加一项。场景定义与 CLI 子命令 `crabmate e2e` 共享。
//!
//! # 运行模式
//!
//! - **默认（无 `REAL_LLM_E2E`）**：`#[ignore]` 跳过所有用例
//! - `REAL_LLM_E2E=1`：真实 LLM 后端，不录制
//! - `REAL_LLM_E2E=1 CM_E2E_RECORD=1`：真实 LLM 后端 + 录制
//!
//! # 输出
//!
//! 每次执行后 artifact 输出到 `.crabmate/e2e_artifacts/<scenario_name>/`，包含：
//! - `metrics.json`：结构化指标
//! - `summary.md`：简短摘要
//! - `messages_final.md` / `messages_final.json`：完整消息记录

use std::path::{Path, PathBuf};

mod common;

use crate::common::E2E_ARTIFACTS_ROOT;

use crabmate::e2e_scenario::{
    E2eRunConfig, TestRunMetrics, TestScenario, generate_report, run_scenario,
};

/// 构建 e2e runner 配置（从环境变量读取模式，与 CLI 默认路径一致）。
fn test_e2e_config() -> E2eRunConfig {
    let mode = match std::env::var("CM_E2E_RECORD") {
        Ok(v) if v == "1" || v.to_lowercase() == "true" => crabmate_llm::E2eMode::Record,
        _ => crabmate_llm::E2eMode::Real,
    };
    E2eRunConfig {
        api_key: resolve_test_api_key(),
        artifacts_root: PathBuf::from(E2E_ARTIFACTS_ROOT),
        recordings_dir: PathBuf::from("tests/fixtures/llm_recordings"),
        mode,
        judge_config: Default::default(),
    }
}

/// 优先从 `API_KEY` 环境变量读取；未设置时回退到 Tauri/Web 本地配置。
fn resolve_test_api_key() -> String {
    let from_env = std::env::var("API_KEY").unwrap_or_default();
    if !from_env.trim().is_empty() {
        return from_env.trim().to_string();
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(&home).join(".local/share"));
    let secret_path = data_home
        .join("crabmate")
        .join("secrets")
        .join("client_llm");
    if let Ok(content) = std::fs::read_to_string(&secret_path) {
        let t = content.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    String::new()
}

/// 单场景 smoke 测试：简单问候，验证一轮 LLM 调用后能正常结束。
///
/// 默认 `#[ignore]`；设置 `REAL_LLM_E2E=1` 时自动启用。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "设置 REAL_LLM_E2E=1 后执行；需先录制或使用真实 LLM"]
async fn e2e_single_agent_smoke() {
    let e2e_cfg = test_e2e_config();
    let metrics = run_scenario(
        &TestScenario {
            name: "orch_single_agent_smoke".to_string(),
            user_message: "你好，用一句话介绍自己。".to_string(),
            workspace_files: vec![],
            expected_output_contains: vec![],
            expected_tool_used: None,
        },
        &e2e_cfg,
    )
    .await;

    assert!(metrics.success, "smoke 测试应成功");
    assert!(metrics.last_role == "assistant", "末条应为 assistant");
    assert!(!metrics.final_output_preview.is_empty(), "回复不应为空");
}

/// 单场景工具调用测试：使用 get_current_time 工具。
///
/// 默认 `#[ignore]`；设置 `REAL_LLM_E2E=1` 时自动启用。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "设置 REAL_LLM_E2E=1 后执行；需先录制或使用真实 LLM"]
async fn e2e_single_agent_tool_round() {
    let e2e_cfg = test_e2e_config();
    let metrics = run_scenario(
        &TestScenario {
            name: "orch_single_agent_tool".to_string(),
            user_message: "请调用 get_current_time 工具查询当前时间，然后用一句话总结。"
                .to_string(),
            workspace_files: vec![],
            expected_output_contains: vec![],
            expected_tool_used: Some("get_current_time".to_string()),
        },
        &e2e_cfg,
    )
    .await;

    assert!(metrics.success, "工具调用测试应成功");
    assert!(metrics.tool_call_count > 0, "应有工具调用");
    assert!(metrics.expected_tool_matched, "应调用了 get_current_time");
    assert!(metrics.last_role == "assistant", "末条应为 assistant");
}

/// 单场景 C++ CMake 项目测试：编译并运行 hello world。
///
/// 默认 `#[ignore]`；设置 `REAL_LLM_E2E=1` 时启用。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "设置 REAL_LLM_E2E=1 后执行；需 cmake + C++ 编译器"]
async fn e2e_single_agent_cpp_cmake_round() {
    let e2e_cfg = test_e2e_config();
    let metrics = run_scenario(
        &TestScenario {
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
        &e2e_cfg,
    )
    .await;

    assert!(metrics.success, "C++ CMake 测试应成功");
    assert!(
        metrics.expected_output_matched,
        "输出应包含程序运行结果或编译成功信息"
    );
    assert!(metrics.expected_tool_matched, "应使用 run_command 工具");
}

/// 统一场景迭代测试：运行所有预设场景，生成汇总报告。
///
/// 默认 `#[ignore]`；设置 `REAL_LLM_E2E=1` 时自动启用。
///
/// # 添加新场景
///
/// 只需在 `scenarios` 列表中添加一项 [`TestScenario`]：
///
/// ```ignore
/// TestScenario {
///     name: "my_new_scenario",
///     user_message: "做某事",
///     workspace_files: &[("file.txt", "content")],
///     expected_output_contains: &["关键词"],
///     expected_tool_used: Some("some_tool"),
/// }
/// ```
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "设置 REAL_LLM_E2E=1 后执行；包含多个 e2e 场景"]
async fn e2e_all_scenarios() {
    let e2e_cfg = test_e2e_config();

    let scenarios = vec![
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
    ];

    let total = scenarios.len();
    let mut results: Vec<TestRunMetrics> = Vec::with_capacity(total);

    for scenario in &scenarios {
        eprintln!(">>> 执行场景: {}", scenario.name);
        let metrics = run_scenario(scenario, &e2e_cfg).await;
        results.push(metrics);
    }

    // 生成报告
    let report_path = PathBuf::from(E2E_ARTIFACTS_ROOT).join("scenario_report.md");
    generate_report(&results, &report_path);

    let json_path = PathBuf::from(E2E_ARTIFACTS_ROOT).join("scenario_report.json");
    if let Ok(json) = serde_json::to_string_pretty(&results) {
        let _ = std::fs::write(&json_path, &json);
    }

    // 断言：所有场景应成功
    for r in &results {
        assert!(r.success, "场景 {} 失败: {:?}", r.scenario, r.error_message);
    }
}

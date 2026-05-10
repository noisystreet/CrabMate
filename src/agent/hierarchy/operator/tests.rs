use crate::agent::context_window::prepare_messages_before_model_call_sync;
use crate::types::{Message, MessageContent};

use super::super::artifact_resolver::ArtifactResolver;
use super::super::artifact_store::ArtifactStore;
use super::super::task::{Artifact, ArtifactKind, SubGoal, TaskStatus};
use super::{OperatorAgent, OperatorConfig};

#[test]
fn react_messages_session_sync_truncates_like_main_loop() {
    let mut cfg = crate::config::load_config(None).expect("embed default");
    cfg.session_ui.max_message_history = 6;
    cfg.tool_transcript.tool_message_max_chars = 1_000_000;
    cfg.context_pipeline.context_char_budget = 0;

    let mut messages = vec![
        Message::system_only("sys".to_string()),
        Message::user_only("task".to_string()),
    ];
    for i in 0..20 {
        messages.push(Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text(format!("step {i}"))),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        });
    }
    let before = messages.len();
    prepare_messages_before_model_call_sync(&mut messages, &cfg);
    assert!(
        messages.len() < before,
        "session sync should drop old rounds when exceeding max_message_history"
    );
    assert_eq!(messages.first().map(|m| m.role.as_str()), Some("system"));
}

#[tokio::test]
async fn test_execute() {
    let config = OperatorConfig::default();
    let operator = OperatorAgent::new(config);
    let goal = SubGoal::new("test", "测试目标").with_tools(vec!["read_file".to_string()]);

    let result = operator.execute(&goal).await.unwrap();
    assert!(matches!(result.status, TaskStatus::Completed));
}

#[test]
fn test_get_tools_for_capabilities() {
    let tools = ["read_file".to_string(), "run_command".to_string()];
    assert!(tools.contains(&"read_file".to_string()));
    assert!(tools.contains(&"run_command".to_string()));
}

#[test]
fn test_is_tool_allowed() {
    let config = OperatorConfig {
        policy: crate::agent::hierarchy::operator::OperatorPolicy {
            max_iterations: 10,
            allowed_tools: vec!["read_file".to_string()],
            tools_defs: vec![],
            enable_compile_error_recovery: true,
            compile_error_max_retries: 3,
            enable_dynamic_decomposition: true,
            dynamic_decomposition_threshold: 40,
        },
        runtime: crate::agent::hierarchy::operator::OperatorRuntimeHandles::default(),
    };
    let operator = OperatorAgent::new(config);

    assert!(operator.is_tool_allowed("read_file"));
    assert!(!operator.is_tool_allowed("write_file"));
}

#[test]
fn test_inject_artifact_paths_into_tool_call() {
    let mut store = ArtifactStore::new();
    store.put(
        Artifact::new(
            "1",
            "main.cpp",
            ArtifactKind::BuildArtifact(
                crate::agent::hierarchy::task::BuildArtifactKind::SourceFile,
            ),
            "goal_1",
        )
        .with_path("/workspace/src/main.cpp"),
    );

    let resolver = ArtifactResolver::new(&store, None);

    let config = OperatorConfig::default();
    let operator = OperatorAgent::new(config);

    let tool_call = crate::types::ToolCall {
        id: "test-1".to_string(),
        typ: "function".to_string(),
        function: crate::types::FunctionCall {
            name: "run_command".to_string(),
            arguments: r#"{"command": "g++", "args": ["{artifact:main.cpp}", "-o", "main"]}"#
                .to_string(),
        },
    };

    let injected = operator.inject_artifact_paths_into_tool_call(&tool_call, &resolver);

    assert!(
        injected
            .function
            .arguments
            .contains("/workspace/src/main.cpp")
    );
    assert!(!injected.function.arguments.contains("{artifact:main.cpp}"));
}

#[test]
fn test_inject_ref_placeholder_into_tool_call() {
    let mut store = ArtifactStore::new();
    store.put(
        Artifact::new(
            "a1",
            "out",
            ArtifactKind::BuildArtifact(
                crate::agent::hierarchy::task::BuildArtifactKind::Executable,
            ),
            "goal_1",
        )
        .with_path("build/prog"),
    );
    let resolver = ArtifactResolver::new(&store, None);
    let operator = OperatorAgent::new(OperatorConfig::default());
    let tool_call = crate::types::ToolCall {
        id: "r1".to_string(),
        typ: "function".to_string(),
        function: crate::types::FunctionCall {
            name: "read_file".to_string(),
            arguments: r#"{"path": "{ref:goal_1:a1}"}"#.to_string(),
        },
    };
    let injected = operator.inject_artifact_paths_into_tool_call(&tool_call, &resolver);
    assert!(injected.function.arguments.contains("build/prog"));
    assert!(!injected.function.arguments.contains("{ref:goal_1:a1}"));
}

#[test]
fn test_inject_paths_into_value_nested() {
    let mut store = ArtifactStore::new();
    store.put(
        Artifact::new(
            "2",
            "test.cpp",
            ArtifactKind::BuildArtifact(
                crate::agent::hierarchy::task::BuildArtifactKind::SourceFile,
            ),
            "goal_2",
        )
        .with_path("/home/user/test.cpp"),
    );

    let resolver = ArtifactResolver::new(&store, None);

    let mut value = serde_json::json!({
        "source": "{artifact:test.cpp}",
        "options": {
            "input": "{artifact:test.cpp}"
        }
    });

    let modified = OperatorAgent::inject_paths_into_value(&mut value, &resolver);

    assert!(modified);
    assert_eq!(value["source"], "/home/user/test.cpp");
    assert_eq!(value["options"]["input"], "/home/user/test.cpp");
}

#[test]
fn test_convergence_goal_detection() {
    let goal = SubGoal::new("g1", "修复编译报错直到 cargo check 通过");
    assert!(super::compile::is_convergence_compile_fix_goal(&goal));
    let goal = SubGoal::new("g2", "编写 README 文档");
    assert!(!super::compile::is_convergence_compile_fix_goal(&goal));
}

#[test]
fn test_parse_compile_error_metrics() {
    let out = "error[E0425]: cannot find value `x`\nwarning: unused variable\nsrc/main.rs:3:5: error: expected `;`";
    let m = super::compile::parse_compile_error_metrics(out).expect("metrics");
    assert_eq!(m.error_count, 2);
    assert!(m.first_error_signature.contains("error"));
}

#[test]
fn test_early_convergence_when_run_build_executable_succeeds() {
    let goal = SubGoal::new("goal_run", "运行 ./build/hello 并验证输出");
    let ok = OperatorAgent::is_successful_build_executable_run_command(
        &goal,
        "run_command",
        r#"{"command":"./build/hello","args":[]}"#,
        true,
    );
    assert!(ok);
}

#[test]
fn test_no_early_convergence_when_only_build_command_succeeds() {
    let goal = SubGoal::new(
        "goal_build",
        "运行 cmake --build build 编译生成可执行文件，不执行程序",
    );
    let ok = OperatorAgent::is_successful_build_executable_run_command(
        &goal,
        "run_command",
        r#"{"command":"cmake","args":["--build","build"]}"#,
        true,
    );
    assert!(!ok);
}

#[test]
fn test_lightweight_dedupe_signature_for_cat_same_file() {
    let sig = OperatorAgent::lightweight_dedupe_signature_for_run_command(
        "run_command",
        r#"{"command":"cat","args":["main.cpp"]}"#,
    );
    assert_eq!(sig.as_deref(), Some("run_command:cat:main.cpp"));
}

#[test]
fn test_lightweight_dedupe_signature_for_ls_same_directory() {
    let sig = OperatorAgent::lightweight_dedupe_signature_for_run_command(
        "run_command",
        r#"{"command":"ls","args":["-la","build"]}"#,
    );
    assert_eq!(sig.as_deref(), Some("run_command:ls:build"));
}

#[test]
fn test_lightweight_cache_hit_for_repeated_cat_in_same_subgoal() {
    use crate::agent::hierarchy::tool_executor::ToolExecutionResult;
    use std::collections::HashMap;

    let mut cache: HashMap<String, ToolExecutionResult> = HashMap::new();
    let key = OperatorAgent::lightweight_dedupe_signature_for_run_command(
        "run_command",
        r#"{"command":"cat","args":["main.cpp"]}"#,
    )
    .expect("dedupe key");
    cache.insert(
        key,
        ToolExecutionResult {
            tool_name: "run_command".to_string(),
            output: "命令：cat main.cpp\n退出码：0\n标准输出：\nHello".to_string(),
            error: None,
            success: true,
            extracted_artifacts: Vec::new(),
        },
    );

    let hit = OperatorAgent::get_lightweight_cached_run_command_result(
        &cache,
        "run_command",
        r#"{"command":"cat","args":["main.cpp"]}"#,
    );
    assert!(hit.is_some());
    let missed = OperatorAgent::get_lightweight_cached_run_command_result(
        &cache,
        "run_command",
        r#"{"command":"cat","args":["other.cpp"]}"#,
    );
    assert!(missed.is_none());
}

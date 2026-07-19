//! `TestServer`：启动真实 axum 实例，支持录制/回放 LLM 后端注入。
//!
//! 根据环境变量自动选择运行模式（`replay` / `record` / `real`）。

use std::path::{Path, PathBuf};

use crabmate::crabmate_llm::{
    ChatCompletionsBackend, E2eMode, build_e2e_backend, detect_mode_from_env,
};

/// e2e 测试服务器句柄。
pub struct TestServer {
    pub base_url: String,
    pub artifacts_dir: PathBuf,
    #[allow(dead_code)]
    pub recordings_dir: PathBuf,
    /// 当前 e2e 运行模式
    #[allow(dead_code)]
    pub mode: E2eMode,
    #[allow(dead_code)]
    handle: Option<crabmate::test_serve::TestServeHandle>,
}

impl TestServer {
    /// 启动测试服务器（随机端口），按环境变量选择运行模式。
    ///
    /// - **默认**：`Replay` 模式，从 `tests/fixtures/llm_recordings/<test_name>/` 回放
    /// - `REAL_LLM_E2E=1`：`Real` 模式，直连真实 LLM
    /// - `REAL_LLM_E2E=1 CM_E2E_RECORD=1`：`Record` 模式，录制到 `tests/fixtures/llm_recordings/<test_name>/`
    ///
    /// `test_name` 用于构造 artifact 目录（`.crabmate/e2e_artifacts/<test_name>/`）和录制目录。
    pub async fn start(test_name: &str) -> Self {
        let mode = detect_mode_from_env();
        let artifacts_dir = Path::new(crate::common::E2E_ARTIFACTS_ROOT).join(test_name);
        let _ = std::fs::create_dir_all(&artifacts_dir);
        let recordings_dir = PathBuf::from("tests/fixtures/llm_recordings");

        // 环境快照
        let _ = std::fs::write(artifacts_dir.join("env.txt"), env_snapshot());

        // 构造 LLM 后端
        let llm_backend = match mode {
            E2eMode::Real | E2eMode::Record => {
                // Real / Record 模式需要真实 LLM（需要 API_KEY）
                let boxed = build_e2e_backend(
                    mode,
                    Box::new(crabmate::OpenAiCompatBackend),
                    &recordings_dir,
                    test_name,
                )
                .expect("构建 e2e 后端失败");
                // Leak 为 &'static（安全：box 由 TestServeHandle 持有）
                let leaked: &'static (dyn ChatCompletionsBackend + 'static) = Box::leak(boxed);
                Some(leaked)
            }
            E2eMode::Replay => {
                // Replay 模式使用 OpenAiCompatBackend 作为占位——实际由 ReplayBackend 接管
                let boxed = build_e2e_backend(
                    mode,
                    Box::new(crabmate::OpenAiCompatBackend),
                    &recordings_dir,
                    test_name,
                )
                .expect("构建 e2e 后端失败；请先用 record 模式录制");
                let leaked: &'static (dyn ChatCompletionsBackend + 'static) = Box::leak(boxed);
                Some(leaked)
            }
        };

        let handle = crabmate::test_serve::start_test_serve(llm_backend).await;
        let base_url = handle.base_url.clone();

        Self {
            base_url,
            artifacts_dir,
            recordings_dir,
            mode,
            handle: Some(handle),
        }
    }

    /// `POST /chat/stream` 快速发送聊天请求。
    pub fn post_chat_stream(&self, body: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .post(format!("{}/chat/stream", self.base_url))
            .header("content-type", "application/json")
            .body(body.to_string())
    }

    /// `POST /chat` 同步聊天。
    #[allow(dead_code)]
    pub fn post_chat(&self, body: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .post(format!("{}/chat", self.base_url))
            .header("content-type", "application/json")
            .body(body.to_string())
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // handle 被 drop → shutdown_tx 关闭 → axum graceful shutdown
    }
}

fn env_snapshot() -> String {
    let mut s = String::new();
    for (k, v) in std::env::vars() {
        if k.contains("KEY") || k.contains("SECRET") || k.contains("TOKEN") {
            s.push_str(&format!("{}=<redacted>\n", k));
        } else {
            s.push_str(&format!("{}={}\n", k, v));
        }
    }
    s
}

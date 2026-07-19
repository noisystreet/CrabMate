//! `TestServer`：启动真实 axum 实例，供 e2e 测试发送 HTTP 请求。
//!
//! 骨架版使用默认 `crabmate::test_serve::start_test_serve`，不注入 LLM 后端。
//! 录制文件落盘由后续 PR 接入。

use std::path::{Path, PathBuf};

/// e2e 测试服务器句柄。
pub struct TestServer {
    pub base_url: String,
    pub artifacts_dir: PathBuf,
    #[allow(dead_code)]
    handle: Option<crabmate::test_serve::TestServeHandle>,
}

impl TestServer {
    /// 启动测试服务器（随机端口）。
    ///
    /// `test_name` 用于构造 artifact 目录路径（`.crabmate/e2e_artifacts/<test_name>/`）。
    pub async fn start(test_name: &str) -> Self {
        let artifacts_dir = Path::new(crate::common::E2E_ARTIFACTS_ROOT).join(test_name);
        let _ = std::fs::create_dir_all(&artifacts_dir);

        // 环境快照
        let _ = std::fs::write(artifacts_dir.join("env.txt"), env_snapshot());

        let handle = crabmate::test_serve::start_test_serve().await;
        let base_url = handle.base_url.clone();

        Self {
            base_url,
            artifacts_dir,
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
        // handle 被 drop 时会触发 shutdown_tx 关闭，axum graceful shutdown 执行
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

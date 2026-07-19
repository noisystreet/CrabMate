//! `TestContext`：e2e 测试上下文，管理 artifact 落盘与 trace sink 生命周期。

use std::sync::Arc;

use crabmate::crabmate_llm::{E2eMode, FileTraceSink, TraceSink};

use super::error_classify;
use super::test_server::TestServer;

/// e2e 测试上下文：封装 `TestServer` + `TraceSink`，在失败时自动 dump 关键信息。
///
/// 骨架和完整版过渡阶段部分字段/方法未使用，但保留完整定义供后续接入。
#[allow(dead_code)]
pub struct TestContext {
    pub server: TestServer,
    pub trace_sink: Option<Arc<dyn TraceSink>>,
}

#[allow(dead_code)]
impl TestContext {
    /// 启动测试并创建 `TraceSink`（`Replay` 模式跳过，`Record`/`Real` 模式创建 `FileTraceSink`）。
    pub async fn start(test_name: &str) -> Self {
        let server = TestServer::start(test_name).await;

        // 仅在真实/录制模式创建 trace sink
        let trace_sink = match server.mode {
            E2eMode::Record | E2eMode::Real => {
                FileTraceSink::create(&server.artifacts_dir, test_name)
                    .ok()
                    .map(|s| Arc::new(s) as Arc<dyn TraceSink>)
            }
            E2eMode::Replay => None,
        };

        Self { server, trace_sink }
    }

    /// 失败时 dump 关键上下文到 artifact 目录。
    pub async fn dump_on_failure(&self, err: &dyn std::error::Error) {
        eprintln!(
            "测试失败: {}\n  artifact: {}",
            err,
            self.server.artifacts_dir.display()
        );

        let report = error_classify::classify(&err.to_string(), &self.server.artifacts_dir);
        let report_json = serde_json::json!({
            "kind": report.kind.as_str(),
            "raw": report.raw,
            "playbook_advice": report.playbook_advice,
        });
        let _ = std::fs::write(
            self.server.artifacts_dir.join("error_report.json"),
            serde_json::to_string_pretty(&report_json).unwrap_or_default(),
        );

        eprintln!(
            "  分类: {:?}\n  建议: {:?}",
            report.kind, report.playbook_advice
        );
    }
}

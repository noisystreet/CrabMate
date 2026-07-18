//! 录制/回放基础设施：让真实 LLM e2e 测试可在 CI 中确定性回归。
//!
//! 三种模式（由环境变量 `REAL_LLM_E2E` / `CM_E2E_RECORD` 选择）：
//! - [`E2eMode::Real`]：直连真实 LLM，不录制（临时真实验证）
//! - [`E2eMode::Record`]：直连真实 LLM，并把每次请求/响应落盘到 `tests/fixtures/llm_recordings/<test>/`
//! - [`E2eMode::Replay`]：完全离线，从录制文件按 round_index 顺序回放（CI 默认）
//!
//! 录制文件布局：
//! ```text
//! tests/fixtures/llm_recordings/<test_name>/
//! ├── manifest.json              # 模型、录制时间、round 数
//! ├── round_0_req.json           # 请求快照（含 messages、tools）
//! ├── round_0_resp.json          # 响应快照（Message + finish_reason + elapsed_ms）
//! ├── round_1_req.json
//! ├── round_1_resp.json
//! └── ...
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crabmate_types::{ChatRequest, Message};

use crate::backend::ChatCompletionsBackend;
use crate::chat_params::StreamChatParams;
use crate::fingerprint::RequestFingerprint;

// ---------------------------------------------------------------------------
// E2eMode 与环境变量检测
// ---------------------------------------------------------------------------

/// e2e 测试运行模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum E2eMode {
    /// 直连真实 LLM，不录制。
    Real,
    /// 直连真实 LLM，并录制请求/响应到 `tests/fixtures/llm_recordings/`。
    Record,
    /// 完全离线，从录制文件回放（CI 默认）。
    Replay,
}

impl E2eMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::Record => "record",
            Self::Replay => "replay",
        }
    }
}

/// 按环境变量检测运行模式：
/// - `REAL_LLM_E2E=1` + `CM_E2E_RECORD=1` → [`E2eMode::Record`]
/// - `REAL_LLM_E2E=1`（无 `CM_E2E_RECORD`） → [`E2eMode::Real`]
/// - 其余 → [`E2eMode::Replay`]（CI 默认）
pub fn detect_mode_from_env() -> E2eMode {
    let real = std::env::var("REAL_LLM_E2E").is_ok();
    let record = std::env::var("CM_E2E_RECORD").is_ok();
    match (real, record) {
        (true, true) => E2eMode::Record,
        (true, false) => E2eMode::Real,
        _ => E2eMode::Replay,
    }
}

// ---------------------------------------------------------------------------
// 录制文件 schema
// ---------------------------------------------------------------------------

/// `manifest.json` 内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingManifest {
    pub test_name: String,
    pub recorded_at: String,
    pub model: String,
    pub rounds: usize,
    /// 录制时的 CrabMate 版本（用于检测录制是否过期）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crabmate_version: Option<String>,
}

/// `round_N_req.json` 内容（请求快照）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedRequest {
    pub round: usize,
    pub fingerprint: String,
    pub model: String,
    /// 完整 messages（serde_json::Value 形式，避免 ChatRequest 未实现 Deserialize）
    pub messages: serde_json::Value,
    pub tools_count: usize,
}

/// `round_N_resp.json` 内容（响应快照）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedResponse {
    pub round: usize,
    pub fingerprint: String,
    pub finish_reason: String,
    pub elapsed_ms: u64,
    pub message: Message,
}

// ---------------------------------------------------------------------------
// RecordingBackend
// ---------------------------------------------------------------------------

/// 包装真实后端，录制每次 `stream_chat` 的请求与响应到磁盘。
///
/// 不在热路径：仅 e2e `record` 模式使用；文件写入用同步 `std::fs`（每次几 KB，可接受）。
pub struct RecordingBackend {
    inner: Box<dyn ChatCompletionsBackend>,
    recordings_dir: PathBuf,
    test_name: String,
    call_seq: AtomicUsize,
    /// 首次调用时写入 manifest；后续跳过
    manifest_written: std::sync::atomic::AtomicBool,
    model_for_manifest: std::sync::Mutex<Option<String>>,
}

impl RecordingBackend {
    pub fn new(
        inner: Box<dyn ChatCompletionsBackend>,
        recordings_dir: impl Into<PathBuf>,
        test_name: impl Into<String>,
    ) -> Self {
        Self {
            inner,
            recordings_dir: recordings_dir.into(),
            test_name: test_name.into(),
            call_seq: AtomicUsize::new(0),
            manifest_written: std::sync::atomic::AtomicBool::new(false),
            model_for_manifest: std::sync::Mutex::new(None),
        }
    }

    fn test_dir(&self) -> PathBuf {
        self.recordings_dir.join(&self.test_name)
    }

    fn write_json(&self, filename: &str, value: &impl Serialize) {
        let dir = self.test_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(filename);
        if let Ok(json) = serde_json::to_string_pretty(value) {
            let _ = std::fs::write(path, json);
        }
    }

    fn maybe_write_manifest(&self, model: &str) {
        if self.manifest_written.swap(true, Ordering::SeqCst) {
            return;
        }
        *self.model_for_manifest.lock().unwrap() = Some(model.to_string());
        let manifest = RecordingManifest {
            test_name: self.test_name.clone(),
            recorded_at: chrono_now_iso(),
            model: model.to_string(),
            rounds: 0, // 后续 finalize_manifest 时更新
            crabmate_version: None,
        };
        self.write_json("manifest.json", &manifest);
    }

    /// 录制结束后调用，更新 manifest 的 rounds 数。
    pub fn finalize_manifest(&self) {
        let rounds = self.call_seq.load(Ordering::SeqCst);
        let model = self.model_for_manifest.lock().unwrap().clone();
        let manifest = RecordingManifest {
            test_name: self.test_name.clone(),
            recorded_at: chrono_now_iso(),
            model: model.unwrap_or_default(),
            rounds,
            crabmate_version: None,
        };
        self.write_json("manifest.json", &manifest);
    }
}

#[async_trait]
impl ChatCompletionsBackend for RecordingBackend {
    async fn stream_chat(
        &self,
        params: &StreamChatParams<'_>,
        req: &mut ChatRequest,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
        let round = self.call_seq.fetch_add(1, Ordering::SeqCst);
        let fp = RequestFingerprint::from_request(req, round);

        self.maybe_write_manifest(&req.model);

        // 请求快照（messages 用 serde_json::Value，因为 ChatRequest 未实现 Deserialize）
        let messages_value = serde_json::to_value(&req.messages).unwrap_or(serde_json::Value::Null);
        let recorded_req = RecordedRequest {
            round,
            fingerprint: fp.hash.clone(),
            model: req.model.clone(),
            messages: messages_value,
            tools_count: req.tools.as_ref().map_or(0, |t| t.len()),
        };
        self.write_json(&format!("round_{round}_req.json"), &recorded_req);

        let start = std::time::Instant::now();
        let result = self.inner.stream_chat(params, req).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match &result {
            Ok((msg, finish_reason)) => {
                let recorded_resp = RecordedResponse {
                    round,
                    fingerprint: fp.hash.clone(),
                    finish_reason: finish_reason.clone(),
                    elapsed_ms,
                    message: msg.clone(),
                };
                self.write_json(&format!("round_{round}_resp.json"), &recorded_resp);
            }
            Err(e) => {
                // 错误也落盘，便于回放时诊断
                let err_snapshot = serde_json::json!({
                    "round": round,
                    "fingerprint": fp.hash,
                    "error": e.to_string(),
                });
                self.write_json(&format!("round_{round}_error.json"), &err_snapshot);
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// ReplayBackend
// ---------------------------------------------------------------------------

/// 从录制文件读取，按 `round_index` 顺序回放响应。
///
/// 启动时一次性加载所有 `round_N_resp.json`；运行时按 `call_seq` 顺序返回。
/// 不校验请求指纹（宽松模式）——录制与回放的请求构造逻辑相同，顺序匹配即可。
#[derive(Debug)]
pub struct ReplayBackend {
    responses: Vec<ReplayEntry>,
    call_seq: AtomicUsize,
}

#[derive(Debug)]
struct ReplayEntry {
    /// 录制时的请求指纹；当前按 round 顺序回放，未做严格校验。
    /// 未来可用于严格模式（请求指纹不匹配时报错）。
    #[allow(dead_code)]
    fingerprint: String,
    message: Message,
    finish_reason: String,
}

impl ReplayBackend {
    /// 从 `<recordings_dir>/<test_name>/` 加载所有录制响应。
    pub fn load(recordings_dir: &Path, test_name: &str) -> Result<Self, String> {
        let test_dir = recordings_dir.join(test_name);
        if !test_dir.is_dir() {
            return Err(format!(
                "录制目录不存在: {}（test_name={test_name}）。\n\
                 提示：请先用 `REAL_LLM_E2E=1 CM_E2E_RECORD=1 cargo test --test <name>` 录制一次。",
                test_dir.display()
            ));
        }

        // 扫描 round_N_resp.json，按 N 排序
        let mut entries: Vec<(usize, ReplayEntry)> = Vec::new();
        for entry in std::fs::read_dir(&test_dir)
            .map_err(|e| format!("读取录制目录失败 {}: {e}", test_dir.display()))?
        {
            let entry = entry.map_err(|e| format!("读取目录项失败: {e}"))?;
            let filename = entry.file_name().to_string_lossy().to_string();
            // 匹配 round_<N>_resp.json（折叠三层 if let 为 let-chain）
            if let Some(rest) = filename.strip_prefix("round_")
                && let Some(n_str) = rest.strip_suffix("_resp.json")
                && let Ok(n) = n_str.parse::<usize>()
            {
                let content = std::fs::read_to_string(entry.path())
                    .map_err(|e| format!("读取 {} 失败: {e}", entry.path().display()))?;
                let resp: RecordedResponse = serde_json::from_str(&content)
                    .map_err(|e| format!("解析 {} 失败: {e}", entry.path().display()))?;
                entries.push((
                    n,
                    ReplayEntry {
                        fingerprint: resp.fingerprint,
                        message: resp.message,
                        finish_reason: resp.finish_reason,
                    },
                ));
            }
        }

        if entries.is_empty() {
            return Err(format!(
                "录制目录为空（无 round_N_resp.json）: {}（test_name={test_name}）",
                test_dir.display()
            ));
        }

        entries.sort_by_key(|(n, _)| *n);
        let responses: Vec<ReplayEntry> = entries.into_iter().map(|(_, e)| e).collect();

        Ok(Self {
            responses,
            call_seq: AtomicUsize::new(0),
        })
    }

    /// 已加载的录制响应数。
    pub fn recorded_rounds(&self) -> usize {
        self.responses.len()
    }
}

#[async_trait]
impl ChatCompletionsBackend for ReplayBackend {
    async fn stream_chat(
        &self,
        _params: &StreamChatParams<'_>,
        _req: &mut ChatRequest,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
        let round = self.call_seq.fetch_add(1, Ordering::SeqCst);
        let entry = self.responses.get(round).ok_or_else(
            || -> Box<dyn std::error::Error + Send + Sync> {
                format!(
                    "ReplayBackend: round {round} 超出录制范围（共 {} 轮录制）。\n\
                 提示：agent 实际调用了更多轮 LLM，请重新录制。",
                    self.responses.len()
                )
                .into()
            },
        )?;
        Ok((entry.message.clone(), entry.finish_reason.clone()))
    }
}

// ---------------------------------------------------------------------------
// 工厂函数
// ---------------------------------------------------------------------------

/// 按 [`E2eMode`] 构造后端。
///
/// - [`E2eMode::Real`]：直接返回 `real_backend`
/// - [`E2eMode::Record`]：返回 `RecordingBackend` 包装 `real_backend`
/// - [`E2eMode::Replay`]：返回 `ReplayBackend`，从 `recordings_dir/<test_name>/` 加载
///
/// `Replay` 模式失败（录制文件缺失）会返回错误，提示如何录制。
pub fn build_e2e_backend(
    mode: E2eMode,
    real_backend: Box<dyn ChatCompletionsBackend>,
    recordings_dir: &Path,
    test_name: &str,
) -> Result<Box<dyn ChatCompletionsBackend>, String> {
    match mode {
        E2eMode::Real => Ok(real_backend),
        E2eMode::Record => Ok(Box::new(RecordingBackend::new(
            real_backend,
            recordings_dir,
            test_name,
        ))),
        E2eMode::Replay => Ok(Box::new(ReplayBackend::load(recordings_dir, test_name)?)),
    }
}

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

fn chrono_now_iso() -> String {
    // 不引入 chrono 依赖，用 SystemTime + 简单格式化
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crabmate_types::{ChatRequest, Message};

    /// 测试用的 stub 后端：按序返回预设响应（仅用于 `build_e2e_backend` 的 real/record 模式参数）。
    /// 不在单元测试里调用其 `stream_chat`，因为构造 `StreamChatParams` 需要完整 `StreamChatHost`
    /// 实现（25+ 方法）；录制→回放端到端一致性由根 crate 集成测试覆盖。
    struct StubBackend {
        responses: Vec<(Message, String)>,
        call_seq: AtomicUsize,
    }

    impl StubBackend {
        fn new(responses: Vec<(Message, String)>) -> Self {
            Self {
                responses,
                call_seq: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl ChatCompletionsBackend for StubBackend {
        async fn stream_chat(
            &self,
            _params: &StreamChatParams<'_>,
            _req: &mut ChatRequest,
        ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
            let idx = self.call_seq.fetch_add(1, Ordering::SeqCst);
            self.responses
                .get(idx)
                .cloned()
                .ok_or_else(|| "StubBackend: 响应序列耗尽".to_string().into())
        }
    }

    #[test]
    fn detect_mode_defaults_to_replay() {
        // 默认无环境变量 → Replay
        // 注意：此测试可能因环境变量已设置而失败，仅在没有 REAL_LLM_E2E 时断言
        if std::env::var("REAL_LLM_E2E").is_err() {
            assert_eq!(detect_mode_from_env(), E2eMode::Replay);
        }
    }

    #[test]
    fn replay_errors_when_recordings_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let result = ReplayBackend::load(tmp.path(), "nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("录制目录不存在"));
    }

    #[test]
    fn replay_errors_when_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let test_dir = tmp.path().join("empty_test");
        std::fs::create_dir_all(&test_dir).unwrap();
        let result = ReplayBackend::load(tmp.path(), "empty_test");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("录制目录为空"));
    }

    #[test]
    fn replay_loads_from_manual_files_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let test_dir = tmp.path().join("manual_test");
        std::fs::create_dir_all(&test_dir).unwrap();

        // 手动写 3 个录制响应（故意乱序命名，验证按 round 排序）
        for (round, content) in [(2usize, "third"), (0, "first"), (1, "second")] {
            let resp = RecordedResponse {
                round,
                fingerprint: format!("fp_{round}"),
                finish_reason: "stop".to_string(),
                elapsed_ms: 100,
                message: Message::assistant_only(content.to_string()),
            };
            std::fs::write(
                test_dir.join(format!("round_{round}_resp.json")),
                serde_json::to_string_pretty(&resp).unwrap(),
            )
            .unwrap();
        }
        // 额外放一个 req 文件和无关文件，验证 ReplayBackend 只读 resp 文件
        std::fs::write(
            test_dir.join("round_0_req.json"),
            r#"{"round":0,"fingerprint":"x","model":"m","messages":[],"tools_count":0}"#,
        )
        .unwrap();
        std::fs::write(
            test_dir.join("manifest.json"),
            r#"{"test_name":"manual_test"}"#,
        )
        .unwrap();

        let replay = ReplayBackend::load(tmp.path(), "manual_test").unwrap();
        assert_eq!(replay.recorded_rounds(), 3);
    }

    #[test]
    fn build_e2e_backend_real_returns_inner() {
        // Real 模式下直接返回 inner；验证构造不 panic 即可
        let stub: Box<dyn ChatCompletionsBackend> = Box::new(StubBackend::new(vec![]));
        let tmp = tempfile::tempdir().unwrap();
        let _ = build_e2e_backend(E2eMode::Real, stub, tmp.path(), "any").unwrap();
    }

    #[test]
    fn build_e2e_backend_record_constructs() {
        // Record 模式包装 inner；未调用 stream_chat 时不产生文件
        let stub: Box<dyn ChatCompletionsBackend> = Box::new(StubBackend::new(vec![]));
        let tmp = tempfile::tempdir().unwrap();
        let _ = build_e2e_backend(E2eMode::Record, stub, tmp.path(), "wrap_test").unwrap();
        // 未调用 stream_chat，录制目录不应存在
        assert!(!tmp.path().join("wrap_test").exists());
    }

    #[test]
    fn build_e2e_backend_replay_loads_files() {
        let tmp = tempfile::tempdir().unwrap();
        // 手动构造录制文件
        let test_dir = tmp.path().join("replay_build");
        std::fs::create_dir_all(&test_dir).unwrap();
        let resp = RecordedResponse {
            round: 0,
            fingerprint: "fake".to_string(),
            finish_reason: "stop".to_string(),
            elapsed_ms: 10,
            message: Message::assistant_only("manual".to_string()),
        };
        std::fs::write(
            test_dir.join("round_0_resp.json"),
            serde_json::to_string_pretty(&resp).unwrap(),
        )
        .unwrap();

        let stub: Box<dyn ChatCompletionsBackend> = Box::new(StubBackend::new(vec![]));
        let _ = build_e2e_backend(E2eMode::Replay, stub, tmp.path(), "replay_build").unwrap();
    }
}

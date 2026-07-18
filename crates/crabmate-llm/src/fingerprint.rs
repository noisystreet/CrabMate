//! LLM 请求指纹：用于录制文件命名与回放匹配。
//!
//! 同一 (model, messages, tools, 采样参数) 的请求视为等价；
//! 忽略 `stream` 等传输层开关（录制与回放可能用不同传输模式）。

use sha2::{Digest, Sha256};

use crabmate_types::ChatRequest;

/// LLM 请求指纹（SHA-256，64 字符 hex）。
#[derive(Debug, Clone)]
pub struct RequestFingerprint {
    /// 64 字符 SHA-256 hex
    pub hash: String,
    pub model: String,
    /// 同一 turn 内的第几次 LLM 调用（0-based）
    pub round_index: usize,
}

impl RequestFingerprint {
    /// 从 `ChatRequest` 计算指纹。
    ///
    /// 仅纳入语义稳定字段：`model` / `messages` / `tools` / `tool_choice` /
    /// `max_tokens` / `temperature` / `seed` / vendor 扩展（`thinking` / `reasoning_effort` 等）。
    /// 忽略 `stream`（传输层开关，录制与回放可能不同）。
    pub fn from_request(req: &ChatRequest, round_index: usize) -> Self {
        let mut hasher = Sha256::new();

        // 核心字段
        hasher.update(b"model=");
        hasher.update(req.model.as_bytes());
        hasher.update(b"\nmessages=");
        // 序列化整个 messages 数组（Message 实现了 Serialize）
        if let Ok(msgs_json) = serde_json::to_string(&req.messages) {
            hasher.update(msgs_json.as_bytes());
        } else {
            // fallback：仅用消息数与 role 摘要
            hasher.update(format!("count={}", req.messages.len()).as_bytes());
            for m in &req.messages {
                hasher.update(b"|role=");
                hasher.update(m.role.as_bytes());
            }
        }

        if let Some(tools) = &req.tools {
            hasher.update(b"\ntools=");
            // 工具列表：序列化完整定义（含 parameters schema）
            if let Ok(tools_json) = serde_json::to_string(tools) {
                hasher.update(tools_json.as_bytes());
            } else {
                hasher.update(format!("count={}", tools.len()).as_bytes());
                for t in tools {
                    hasher.update(b"|name=");
                    hasher.update(t.function.name.as_bytes());
                }
            }
        }

        if let Some(tc) = &req.tool_choice {
            hasher.update(b"\ntool_choice=");
            hasher.update(tc.as_bytes());
        }

        hasher.update(b"\nmax_tokens=");
        hasher.update(req.max_tokens.to_string().as_bytes());
        hasher.update(b"\ntemperature=");
        // temperature 用固定精度避免浮点序列化漂移
        hasher.update(format!("{:.6}", req.temperature).as_bytes());

        if let Some(seed) = req.seed {
            hasher.update(b"\nseed=");
            hasher.update(seed.to_string().as_bytes());
        }

        // vendor 扩展（thinking / reasoning_effort / reasoning_split / response_format）
        // 这些字段影响模型行为，必须纳入指纹
        if let Ok(vendor_json) = serde_json::to_string(&req.vendor)
            && vendor_json != "{}"
        {
            hasher.update(b"\nvendor=");
            hasher.update(vendor_json.as_bytes());
        }

        hasher.update(b"\nround=");
        hasher.update(round_index.to_string().as_bytes());

        // sha2 的 finalize() 返回 GenericArray<u8, ...>，不实现 LowerHex；手动转 hex
        let digest = hasher.finalize();
        let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        Self {
            hash,
            model: req.model.clone(),
            round_index,
        }
    }

    /// 短指纹（前 12 字符），用于文件名前缀。
    pub fn short(&self) -> &str {
        // hash 是 64 字符 hex，取前 12 足够避免碰撞
        &self.hash[..12]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crabmate_types::{ChatRequest, ChatRequestCore, ChatRequestVendorExtensions, Message};

    fn sample_request(model: &str) -> ChatRequest {
        ChatRequest {
            core: ChatRequestCore {
                model: model.to_string(),
                messages: vec![
                    Message::system_only("sys".to_string()),
                    Message::user_only("hello".to_string()),
                ],
                tools: None,
                tool_choice: None,
                max_tokens: 128,
                temperature: 0.7,
                seed: Some(42),
                stream: None,
            },
            vendor: ChatRequestVendorExtensions::default(),
        }
    }

    #[test]
    fn fingerprint_stable_for_same_request() {
        let req = sample_request("deepseek-chat");
        let fp1 = RequestFingerprint::from_request(&req, 0);
        let fp2 = RequestFingerprint::from_request(&req, 0);
        assert_eq!(fp1.hash, fp2.hash, "相同请求指纹必须一致");
        assert_eq!(fp1.short().len(), 12);
    }

    #[test]
    fn fingerprint_differs_on_model() {
        let fp1 = RequestFingerprint::from_request(&sample_request("model-a"), 0);
        let fp2 = RequestFingerprint::from_request(&sample_request("model-b"), 0);
        assert_ne!(fp1.hash, fp2.hash, "不同模型指纹必须不同");
    }

    #[test]
    fn fingerprint_differs_on_round_index() {
        let req = sample_request("deepseek-chat");
        let fp1 = RequestFingerprint::from_request(&req, 0);
        let fp2 = RequestFingerprint::from_request(&req, 1);
        assert_ne!(fp1.hash, fp2.hash, "不同 round 指纹必须不同");
    }

    #[test]
    fn fingerprint_ignores_stream_field() {
        let mut req1 = sample_request("deepseek-chat");
        req1.core.stream = Some(true);
        let mut req2 = sample_request("deepseek-chat");
        req2.core.stream = Some(false);
        // stream 不纳入指纹，故应一致
        let fp1 = RequestFingerprint::from_request(&req1, 0);
        let fp2 = RequestFingerprint::from_request(&req2, 0);
        assert_eq!(fp1.hash, fp2.hash, "stream 字段不应影响指纹（传输层开关）");
    }

    #[test]
    fn fingerprint_differs_on_messages() {
        let mut req1 = sample_request("deepseek-chat");
        let mut req2 = sample_request("deepseek-chat");
        req1.core
            .messages
            .push(Message::user_only("extra".to_string()));
        req2.core
            .messages
            .push(Message::user_only("different".to_string()));
        let fp1 = RequestFingerprint::from_request(&req1, 0);
        let fp2 = RequestFingerprint::from_request(&req2, 0);
        assert_ne!(fp1.hash, fp2.hash, "不同消息内容指纹必须不同");
    }
}

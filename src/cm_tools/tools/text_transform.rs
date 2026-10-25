//! 纯内存字符串变换：Base64、URL 百分号编解码、短哈希、按行合并/按分隔符切分（不落盘）。

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64_ENGINE;

use crate::cm_tools::tool_result::ToolError;

/// 单次输入上限（字节）
const MAX_INPUT_BYTES: usize = 256 * 1024;
/// 输出上限（字节）；超出则截断并附说明
const MAX_OUTPUT_BYTES: usize = 512 * 1024;
/// `lines_split` 最多段数
const MAX_SPLIT_PARTS: usize = 50_000;
/// 分隔符最大长度（字节）
const MAX_DELIMITER_BYTES: usize = 256;
/// `base64_decode` 非 UTF-8 时附带的十六进制预览长度
const NON_UTF8_HEX_BYTES: usize = 256;

fn truncate_output(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        return s.to_string();
    }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n\n[输出已截断：共 {} 字节，上限 {} 字节]",
        &s[..end],
        s.len(),
        MAX_OUTPUT_BYTES
    )
}

#[allow(clippy::result_large_err)]
fn validate_text(text: &str) -> Result<(), ToolError> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(ToolError::invalid_args(format!(
            "text 过长：{} 字节，上限 {}",
            text.len(),
            MAX_INPUT_BYTES
        )));
    }
    Ok(())
}

fn hash_short_hex(text: &str) -> String {
    use sha2::Digest;
    let d = sha2::Sha256::digest(text.as_bytes());
    let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
    hex[..16].to_string()
}

/// Base64 解码；结果非 UTF-8 时附十六进制预览（**仍算成功**，与迁移前一致）。
#[allow(clippy::result_large_err)]
fn decode_base64_text(text: &str) -> Result<String, ToolError> {
    let raw = B64_ENGINE
        .decode(text.trim().as_bytes())
        .map_err(|e| ToolError::invalid_args(format!("Base64 解码失败：{e}")))?;
    match String::from_utf8(raw.clone()) {
        Ok(s) => Ok(s),
        Err(_) => {
            let n = NON_UTF8_HEX_BYTES.min(raw.len());
            let hex = hex::encode(&raw[..n]);
            Ok(format!(
                "（解码结果非 UTF-8 文本；以下为前 {} 字节的十六进制，共 {} 字节）\n{}",
                n,
                raw.len(),
                hex
            ))
        }
    }
}

/// 解析 `lines_join` / `lines_split` 分隔符；`allow_empty = false` 时拒绝缺省（split 语义）。
#[allow(clippy::result_large_err)]
fn resolve_delimiter(delim: Option<&str>, allow_empty: bool) -> Result<String, ToolError> {
    match delim {
        None | Some("") if allow_empty => Ok(" ".to_string()),
        None | Some("") => Err(ToolError::invalid_args(
            "lines_split 必须提供非空 delimiter".to_string(),
        )),
        Some(s) if s.len() > MAX_DELIMITER_BYTES => Err(ToolError::invalid_args(format!(
            "delimiter 过长：{} 字节，上限 {}",
            s.len(),
            MAX_DELIMITER_BYTES
        ))),
        Some(s) => Ok(s.to_string()),
    }
}

/// 执行 `text_transform` 工具：入参校验 / 超限 / 解码失败均为 `invalid_args`。
#[allow(clippy::result_large_err)]
pub fn run_try(args_json: &str) -> Result<String, ToolError> {
    let args: super::tool_param_types::TextTransformArgs =
        super::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    validate_text(&args.text)?;
    let text = &args.text;

    let out = match args.op {
        super::tool_param_types::TextTransformOp::Base64Encode => {
            B64_ENGINE.encode(text.as_bytes())
        }
        super::tool_param_types::TextTransformOp::Base64Decode => decode_base64_text(text)?,
        super::tool_param_types::TextTransformOp::UrlEncode => {
            urlencoding::encode(text).into_owned()
        }
        super::tool_param_types::TextTransformOp::UrlDecode => urlencoding::decode(text)
            .map_err(|e| ToolError::invalid_args(format!("URL 解码失败：{e}")))?
            .into_owned(),
        super::tool_param_types::TextTransformOp::HashShort => {
            format!("sha256:{}", hash_short_hex(text))
        }
        super::tool_param_types::TextTransformOp::LinesJoin => {
            let delim = resolve_delimiter(args.delimiter.as_deref(), true)?;
            let lines: Vec<&str> = text.lines().collect();
            lines.join(&delim)
        }
        super::tool_param_types::TextTransformOp::LinesSplit => {
            let delim = resolve_delimiter(args.delimiter.as_deref(), false)?;
            let parts: Vec<&str> = text.split(&delim).collect();
            if parts.len() > MAX_SPLIT_PARTS {
                return Err(ToolError::invalid_args(format!(
                    "切分后段数 {} 超过上限 {}",
                    parts.len(),
                    MAX_SPLIT_PARTS
                )));
            }
            parts.join("\n")
        }
    };

    Ok(truncate_output(&out))
}

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0xf) as usize] as char);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_str(args_json: &str) -> String {
        match run_try(args_json) {
            Ok(s) => s,
            Err(e) => e.message,
        }
    }

    #[test]
    fn base64_roundtrip() {
        let j = serde_json::json!({
            "op": "base64_encode",
            "text": "hello"
        });
        let enc = run_str(&j.to_string());
        let j2 = serde_json::json!({
            "op": "base64_decode",
            "text": enc.trim()
        });
        let dec = run_str(&j2.to_string());
        assert!(dec.contains("hello"));
    }

    #[test]
    fn hash_short_len() {
        let j = serde_json::json!({
            "op": "hash_short",
            "text": "x",
            "hash_algo": "sha256"
        });
        let s = run_str(&j.to_string());
        assert!(s.starts_with("sha256:"));
        assert_eq!(s.len(), "sha256:".len() + 16);
    }

    #[test]
    fn lines_join_split() {
        let j = serde_json::json!({
            "op": "lines_join",
            "text": "a\nb\nc",
            "delimiter": "|"
        });
        assert_eq!(run_str(&j.to_string()), "a|b|c");
        let j2 = serde_json::json!({
            "op": "lines_split",
            "text": "a|b|c",
            "delimiter": "|"
        });
        assert_eq!(run_str(&j2.to_string()), "a\nb\nc");
    }

    #[test]
    fn oversized_text_is_invalid_args() {
        let big = "x".repeat(MAX_INPUT_BYTES + 1);
        let j = serde_json::json!({ "op": "hash_short", "text": big });
        let err = run_try(&j.to_string()).expect_err("oversized text must fail");
        assert_eq!(err.code, "invalid_args");
        assert!(err.message.contains("text 过长"), "{}", err.message);
    }

    #[test]
    fn lines_split_without_delimiter_is_invalid_args() {
        let j = serde_json::json!({ "op": "lines_split", "text": "a|b" });
        let err = run_try(&j.to_string()).expect_err("missing delimiter must fail");
        assert_eq!(err.code, "invalid_args");
        assert!(
            err.message.contains("必须提供非空 delimiter"),
            "{}",
            err.message
        );
    }
}

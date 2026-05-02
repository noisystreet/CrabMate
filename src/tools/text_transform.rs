//! 纯内存字符串变换：Base64、URL 百分号编解码、短哈希、按行合并/按分隔符切分（不落盘）。

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64_ENGINE;

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

fn validate_text(text: &str) -> Result<(), String> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "text 过长：{} 字节，上限 {}",
            text.len(),
            MAX_INPUT_BYTES
        ));
    }
    Ok(())
}

fn hash_short_hex(text: &str, algo: &str) -> String {
    match algo {
        "blake3" => {
            let h = blake3::hash(text.as_bytes());
            let hex = h.to_hex();
            hex.as_str()[..16].to_string()
        }
        _ => {
            use sha2::Digest;
            let d = sha2::Sha256::digest(text.as_bytes());
            format!("{:x}", d)[..16].to_string()
        }
    }
}

/// 执行 `text_transform` 工具。
pub fn run(args_json: &str) -> String {
    let args: super::tool_param_types::TextTransformArgs = match serde_json::from_str(args_json) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 无效: {e}"),
    };
    if let Err(e) = validate_text(&args.text) {
        return e;
    }
    let text = &args.text;

    let out = match args.op {
        super::tool_param_types::TextTransformOp::Base64Encode => {
            B64_ENGINE.encode(text.as_bytes())
        }
        super::tool_param_types::TextTransformOp::Base64Decode => {
            let raw = match B64_ENGINE.decode(text.trim().as_bytes()) {
                Ok(b) => b,
                Err(e) => return format!("Base64 解码失败：{}", e),
            };
            match String::from_utf8(raw.clone()) {
                Ok(s) => s,
                Err(_) => {
                    let n = NON_UTF8_HEX_BYTES.min(raw.len());
                    let hex = hex::encode(&raw[..n]);
                    format!(
                        "（解码结果非 UTF-8 文本；以下为前 {} 字节的十六进制，共 {} 字节）\n{}",
                        n,
                        raw.len(),
                        hex
                    )
                }
            }
        }
        super::tool_param_types::TextTransformOp::UrlEncode => {
            urlencoding::encode(text).into_owned()
        }
        super::tool_param_types::TextTransformOp::UrlDecode => match urlencoding::decode(text) {
            Ok(c) => c.into_owned(),
            Err(e) => return format!("URL 解码失败：{}", e),
        },
        super::tool_param_types::TextTransformOp::HashShort => {
            let algo = match args.hash_algo.unwrap_or_default() {
                super::tool_param_types::TextTransformHashAlgo::Sha256 => "sha256",
                super::tool_param_types::TextTransformHashAlgo::Blake3 => "blake3",
            };
            format!("{}:{}", algo, hash_short_hex(text, algo))
        }
        super::tool_param_types::TextTransformOp::LinesJoin => {
            let delim = match args.delimiter.as_deref() {
                None | Some("") => " ".to_string(),
                Some(s) => {
                    if s.len() > MAX_DELIMITER_BYTES {
                        return format!(
                            "delimiter 过长：{} 字节，上限 {}",
                            s.len(),
                            MAX_DELIMITER_BYTES
                        );
                    }
                    s.to_string()
                }
            };
            let lines: Vec<&str> = text.lines().collect();
            lines.join(&delim)
        }
        super::tool_param_types::TextTransformOp::LinesSplit => {
            let delim = match args.delimiter.as_deref() {
                Some(s) if !s.is_empty() => {
                    if s.len() > MAX_DELIMITER_BYTES {
                        return format!(
                            "delimiter 过长：{} 字节，上限 {}",
                            s.len(),
                            MAX_DELIMITER_BYTES
                        );
                    }
                    s.to_string()
                }
                _ => return "lines_split 必须提供非空 delimiter".to_string(),
            };
            let parts: Vec<&str> = text.split(&delim).collect();
            if parts.len() > MAX_SPLIT_PARTS {
                return format!("切分后段数 {} 超过上限 {}", parts.len(), MAX_SPLIT_PARTS);
            }
            parts.join("\n")
        }
    };

    truncate_output(&out)
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

    #[test]
    fn base64_roundtrip() {
        let j = serde_json::json!({
            "op": "base64_encode",
            "text": "hello"
        });
        let enc = run(&j.to_string());
        let j2 = serde_json::json!({
            "op": "base64_decode",
            "text": enc.trim()
        });
        let dec = run(&j2.to_string());
        assert!(dec.contains("hello"));
    }

    #[test]
    fn hash_short_len() {
        let j = serde_json::json!({
            "op": "hash_short",
            "text": "x",
            "hash_algo": "sha256"
        });
        let s = run(&j.to_string());
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
        assert_eq!(run(&j.to_string()), "a|b|c");
        let j2 = serde_json::json!({
            "op": "lines_split",
            "text": "a|b|c",
            "delimiter": "|"
        });
        assert_eq!(run(&j2.to_string()), "a\nb\nc");
    }
}

//! 飞书事件订阅 **Encrypt Key** 加密体解密（HTTP Webhook）。
//!
//! 算法与官方文档一致：[事件解密](https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/encrypt-key-encryption-configuration-case?lang=zh-CN)
//! — `key = SHA256(encrypt_key_string_utf8)`（32 字节），密文为 **`base64(iv_16_bytes || aes256_cbc_ciphertext)`**，填充 **PKCS#7**。

use aes::Aes256;
use base64::Engine;
use cbc::Decryptor;
use cipher::block_padding::Pkcs7;
use cipher::{BlockModeDecrypt, KeyIvInit};
use sha2::{Digest, Sha256};

type Aes256CbcDec = Decryptor<Aes256>;

#[derive(Debug, thiserror::Error)]
pub enum FeishuDecryptError {
    #[error("FEISHU_ENCRYPT_KEY is not set; cannot decrypt encrypted event")]
    MissingEncryptKey,
    #[error("base64 decode: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("ciphertext too short")]
    CipherTooShort,
    #[error("ciphertext length is not a multiple of AES block size")]
    BadBlockLength,
    #[error("AES decrypt / unpad: {0}")]
    Unpad(String),
    #[error("outer JSON: {0}")]
    OuterJson(#[from] serde_json::Error),
}

/// 若 JSON 仅含 **`encrypt`**（Base64 字符串），用 **Encrypt Key** 解密得到 UTF-8 明文 JSON；否则返回 `None`（调用方继续按明文解析）。
pub fn maybe_decrypt_event_json(
    encrypt_key: Option<&str>,
    body_str: &str,
) -> Result<Option<String>, FeishuDecryptError> {
    let v: serde_json::Value = serde_json::from_str(body_str)?;
    let enc = match v.get("encrypt").and_then(|e| e.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return Ok(None),
    };
    let Some(key_str) = encrypt_key.filter(|k| !k.trim().is_empty()) else {
        return Err(FeishuDecryptError::MissingEncryptKey);
    };
    let plain = decrypt_encrypt_field(key_str, enc)?;
    Ok(Some(plain))
}

/// 解密飞书 **`encrypt`** 字段（Base64）。
pub fn decrypt_encrypt_field(
    encrypt_key: &str,
    encrypt_b64: &str,
) -> Result<String, FeishuDecryptError> {
    let raw = base64::engine::general_purpose::STANDARD.decode(encrypt_b64.trim())?;
    if raw.len() < 16 {
        return Err(FeishuDecryptError::CipherTooShort);
    }
    let (iv, ct) = raw.split_at(16);
    if ct.is_empty() || ct.len() % 16 != 0 {
        return Err(FeishuDecryptError::BadBlockLength);
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&Sha256::digest(encrypt_key.as_bytes()));

    let mut buf = ct.to_vec();
    let dec = Aes256CbcDec::new_from_slices(&key, iv)
        .map_err(|e| FeishuDecryptError::Unpad(format!("AES key/iv length: {e}")))?;
    let plain_bytes = dec
        .decrypt_padded::<Pkcs7>(&mut buf)
        .map_err(|e| FeishuDecryptError::Unpad(format!("{e:?}")))?;
    let plain = std::str::from_utf8(plain_bytes)
        .map_err(|_| FeishuDecryptError::Unpad("decrypted payload is not UTF-8".into()))?;
    Ok(trim_json_noise(plain))
}

/// 与部分官方示例一致：从解密缓冲中提取 `{` … `}` 之间的 JSON（去除前导噪声字节）。
fn trim_json_noise(s: &str) -> String {
    let b = s.as_bytes();
    let start = b.iter().position(|&c| c == b'{').unwrap_or(0);
    let end = b
        .iter()
        .rposition(|&c| c == b'}')
        .map(|i| i + 1)
        .unwrap_or(b.len());
    if start < end {
        s[start..end].to_string()
    } else {
        s.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 飞书文档示例：key `test key`、密文解密后为含 `hello world` 的 JSON。
    #[test]
    fn decrypt_official_sample() {
        let enc = "P37w+VZImNgPEO1RBhJ6RtKl7n6zymIbEG1pReEzghk=";
        let s = decrypt_encrypt_field("test key", enc).expect("decrypt");
        assert!(s.contains("hello world"), "got: {s}");
    }
}

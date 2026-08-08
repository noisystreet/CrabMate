//! 移动端连接页 → 远程 UI 的一次性鉴权交接（URL hash，避免跨源 localStorage）。
//!
//! 连接页导航到 `{serve}/#cm_web_api_bearer=<urlencoded>`；本模块在前端启动时消费并写入
//! [`super::browser::set_web_api_bearer_token`]，再从地址栏清除 token（`history.replaceState`）。
//!
//! **勿**把 Bearer 写入查询串（易进访问日志）；hash 仍可能进 WebView 历史，仅作短时手递。

use super::browser::{set_web_api_bearer_token, window};

/// Hash 参数名（与 Client 仓 connect 页 / `crabmate-connect` 一致）。
pub const CM_WEB_API_BEARER_HASH_KEY: &str = "cm_web_api_bearer";

/// 从 `location.hash`（`#a=1&b=2`）解析键值；值已做 URL 解码。
#[must_use]
pub fn parse_hash_params(hash: &str) -> Vec<(String, String)> {
    let raw = hash.trim().trim_start_matches('#');
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split('&')
        .filter_map(|pair| {
            let pair = pair.trim();
            if pair.is_empty() {
                return None;
            }
            let (k, v) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            let k = urlencoding::decode(k)
                .unwrap_or_else(|_| k.into())
                .into_owned();
            let v = urlencoding::decode(v)
                .unwrap_or_else(|_| v.into())
                .into_owned();
            if k.is_empty() { None } else { Some((k, v)) }
        })
        .collect()
}

/// 去掉敏感键后重建 hash（无参数时返回空串，不含 `#`）。
#[must_use]
pub fn hash_without_bearer(hash: &str) -> String {
    let kept: Vec<(String, String)> = parse_hash_params(hash)
        .into_iter()
        .filter(|(k, _)| k != CM_WEB_API_BEARER_HASH_KEY)
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    kept.into_iter()
        .map(|(k, v)| {
            if v.is_empty() {
                urlencoding::encode(&k).into_owned()
            } else {
                format!("{}={}", urlencoding::encode(&k), urlencoding::encode(&v))
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// 若 hash 含 Bearer：写入本页凭证并 `replaceState` 清掉地址栏中的 token。
///
/// 在 WASM App 启动尽早调用（先于依赖鉴权的 `/status` 拉取）。
pub fn consume_mobile_connect_handoff() {
    let Some(w) = window() else {
        return;
    };
    let loc = w.location();
    let Ok(hash) = loc.hash() else {
        return;
    };
    let params = parse_hash_params(&hash);
    let Some((_, bearer)) = params.iter().find(|(k, _)| k == CM_WEB_API_BEARER_HASH_KEY) else {
        return;
    };
    // 空串也消费：用于明确「无 Bearer」并清掉残留 hash 键
    set_web_api_bearer_token(bearer);

    let new_hash = hash_without_bearer(&hash);
    let Ok(path) = loc.pathname() else {
        return;
    };
    let search = loc.search().unwrap_or_default();
    let next = if new_hash.is_empty() {
        format!("{path}{search}")
    } else {
        format!("{path}{search}#{new_hash}")
    };
    if let Ok(hist) = w.history() {
        let _ = hist.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&next));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bearer_and_strip() {
        let h = "#cm_web_api_bearer=sec%2Fret&cm_shell=mobile";
        let p = parse_hash_params(h);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].0, CM_WEB_API_BEARER_HASH_KEY);
        assert_eq!(p[0].1, "sec/ret");
        assert_eq!(hash_without_bearer(h), "cm_shell=mobile");
    }

    #[test]
    fn empty_hash() {
        assert!(parse_hash_params("").is_empty());
        assert!(parse_hash_params("#").is_empty());
        assert_eq!(hash_without_bearer("#cm_web_api_bearer=abc"), "");
    }
}

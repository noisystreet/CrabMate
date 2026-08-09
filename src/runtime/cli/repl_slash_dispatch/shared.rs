//! REPL 斜杠命令共用的 `--no-persist` 解析与 user-data 持久化。

/// 去掉尾部 `--no-persist`；默认 `persist=true`（写 user-data）。
pub(super) fn split_optional_no_persist(raw: &str) -> (String, bool) {
    let t = raw.trim();
    if let Some(rest) = t.strip_suffix("--no-persist") {
        (rest.trim_end().to_string(), false)
    } else {
        (t.to_string(), true)
    }
}

pub(super) fn persist_client_llm_overrides(
    model: Option<&str>,
    api_base: Option<&str>,
) -> Result<(), String> {
    let mut file = crate::user_data::load_llm_overrides();
    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        file.client_llm.model = Some(m.to_string());
    }
    if let Some(b) = api_base.map(str::trim).filter(|s| !s.is_empty()) {
        file.client_llm.api_base = Some(b.to_string());
    }
    crate::user_data::save_llm_overrides(&file)
}

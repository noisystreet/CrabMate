//! 外网访问类工具的 `CM_*` 覆盖：`web_search.*` / `http_fetch.*` / `weather_tool.*`。
//!
//! 三者是同一类「对外取值」配置（各自的超时、限额、凭据），故同处一个模块；
//! 字段仍分属三个不同配置段，组内顺序不影响结果。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{
    apply_csv_allow_empty, apply_nonempty_opt, apply_parse, apply_raw_opt,
};

pub(super) fn apply(b: &mut ConfigBuilder) {
    web_search_keys(b);
    web_search_limits(b);
    http_fetch_limits(b);
    weather_timeout(b);
}

fn web_search_keys(b: &mut ConfigBuilder) {
    apply_nonempty_opt(
        &mut b.web_search.web_search_provider_str,
        "CM_WEB_SEARCH_PROVIDER",
    );
    apply_raw_opt(
        &mut b.web_search.web_search_api_key,
        "CM_WEB_SEARCH_API_KEY",
    );
    apply_raw_opt(
        &mut b.web_search.web_search_proxy,
        "CM_WEB_SEARCH_PROXY",
    );
}

fn web_search_limits(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.web_search.web_search_timeout_secs,
        "CM_WEB_SEARCH_TIMEOUT_SECS",
    );
    apply_parse(
        &mut b.web_search.web_search_max_results,
        "CM_WEB_SEARCH_MAX_RESULTS",
    );
}

fn http_fetch_limits(b: &mut ConfigBuilder) {
    apply_csv_allow_empty(
        &mut b.http_fetch.http_fetch_allowed_prefixes,
        "CM_HTTP_FETCH_ALLOWED_PREFIXES",
    );
    apply_parse(
        &mut b.http_fetch.http_fetch_timeout_secs,
        "CM_HTTP_FETCH_TIMEOUT_SECS",
    );
    apply_parse(
        &mut b.http_fetch.http_fetch_max_response_bytes,
        "CM_HTTP_FETCH_MAX_RESPONSE_BYTES",
    );
    apply_nonempty_opt(
        &mut b.http_fetch.http_fetch_user_agent,
        "CM_HTTP_FETCH_USER_AGENT",
    );
}

fn weather_timeout(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.weather_tool.weather_timeout_secs,
        "CM_WEATHER_TIMEOUT_SECS",
    );
}

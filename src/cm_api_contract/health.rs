//! `GET /health` 响应：健康检查公开视图（与线上 JSON 1:1）。
//!
//! 服务端 `cm_internal::health::HealthReport` 仅 `Serialize` 且随 `server` feature 编译；
//! 本模块为 protocol 可见的线契约视图，由 handler 做一对一转换（形状不变）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 单项健康检查结果（`checks` 的值；`detail` 为 `None` 时出站省略该键）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HealthCheckItemView {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// `GET /health` 响应体（`status` + `checks`）。
///
/// **不**使用 `deny_unknown_fields`：`checks` 为开放 map，`dep_*` 等检查键随运行环境与
/// 配置动态增减；顶层也容忍未来新增字段，旧 Client 反序列化不破坏。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HealthReportView {
    pub status: String,
    #[serde(default)]
    pub checks: BTreeMap<String, HealthCheckItemView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_report_view_roundtrips_and_omits_absent_detail() {
        let mut checks = BTreeMap::new();
        checks.insert(
            "workspace_writable".to_string(),
            HealthCheckItemView {
                ok: true,
                detail: None,
            },
        );
        checks.insert(
            "dep_bc".to_string(),
            HealthCheckItemView {
                ok: false,
                detail: Some("missing".into()),
            },
        );
        let view = HealthReportView {
            status: "degraded".into(),
            checks,
        };

        let v = serde_json::to_value(&view).expect("serialize");
        assert_eq!(
            v.pointer("/status").and_then(|s| s.as_str()),
            Some("degraded")
        );
        let ws = v
            .pointer("/checks/workspace_writable")
            .and_then(|s| s.as_object())
            .expect("workspace_writable object");
        assert!(ws.get("detail").is_none(), "absent detail must be omitted");
        assert_eq!(
            v.pointer("/checks/dep_bc/detail").and_then(|d| d.as_str()),
            Some("missing")
        );

        let round: HealthReportView = serde_json::from_value(v).expect("round-trip");
        assert_eq!(round.status, view.status);
        assert_eq!(round.checks.len(), 2);
        assert_eq!(round.checks["workspace_writable"].detail, None);
        assert_eq!(round.checks["dep_bc"].detail.as_deref(), Some("missing"));
    }

    #[test]
    fn health_report_view_tolerates_unknown_fields_and_dynamic_check_keys() {
        // dep_* 键随运行环境动态增减；顶层未来字段不破坏旧 Client 反序列化。
        let v = serde_json::json!({
            "status": "degraded",
            "checks": {
                "workspace_writable": { "ok": true },
                "dep_some_future_cli": { "ok": false, "detail": "No such file" }
            },
            "future_top_level_field": 1
        });
        let view: HealthReportView = serde_json::from_value(v).expect("tolerant parse");
        assert_eq!(view.status, "degraded");
        assert!(view.checks["workspace_writable"].ok);
        assert_eq!(view.checks["workspace_writable"].detail, None);
        assert!(!view.checks["dep_some_future_cli"].ok);
    }
}

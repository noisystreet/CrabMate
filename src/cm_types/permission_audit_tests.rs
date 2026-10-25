//! `permission_audit` 的语义钉：哪些结论要留痕、字段串不漂移、`path_scope = any` 只在 L4。

use super::test_support::captured_with;
use super::*;
use crate::cm_types::PermissionLevel;

#[test]
fn axis_and_decision_strings_are_stable() {
    // 对外可检索的字面量，改动等同于破坏日志契约。
    let axes = [
        (PermissionAxis::Tool, "tool"),
        (PermissionAxis::Path, "path"),
        (PermissionAxis::Command, "command"),
        (PermissionAxis::Net, "net"),
        (PermissionAxis::External, "external"),
    ];
    for (axis, expected) in axes {
        assert_eq!(axis.as_str(), expected);
    }
    let decisions = [
        (PermissionDecision::Allowed, "allowed"),
        (PermissionDecision::Denied, "denied"),
        (PermissionDecision::Approved, "approved"),
    ];
    for (decision, expected) in decisions {
        assert_eq!(decision.as_str(), expected);
    }
}

#[test]
fn should_emit_matrix() {
    use ApprovalPolicy::{AuditOnly, Deny, Gate};
    use PermissionDecision::{Allowed, Approved, Denied};
    // (approval, decision, 期望)
    let matrix = [
        (Deny, Denied, true),
        (Deny, Approved, true),
        (Deny, Allowed, false),
        (Gate, Denied, true),
        (Gate, Approved, true),
        (Gate, Allowed, false),
        (AuditOnly, Denied, true),
        (AuditOnly, Approved, true),
        // 只有 L4 的「直接放行」值得留痕——这正是 AuditOnly 的全部意义。
        (AuditOnly, Allowed, true),
    ];
    for (approval, decision, expected) in matrix {
        assert_eq!(
            should_emit(approval, decision),
            expected,
            "approval={} decision={}",
            approval.as_str(),
            decision.as_str()
        );
    }
}

#[test]
fn only_unrestricted_projects_audit_only() {
    // 与 `project_caps` 的一致性由 `cm_agent` 侧的 golden 钉住；这里只钉「对外轴名」这一层，
    // 让本模块不必反向依赖 `cm_agent`。只断言「谁是 audit_only」，不复述其余等级的具体值
    // （那会变成第二张投影表，改一处漏一处）。
    for level in PermissionLevel::ALL {
        assert_eq!(
            level.caps_view().approval == "audit_only",
            level == PermissionLevel::Unrestricted,
            "{level} 的 approval 轴"
        );
    }
}

#[test]
fn path_scope_any_exists_only_at_unrestricted() {
    // `PathAccess` 在 `scope.is_unrestricted()` 时把等级回填为 L4（它拿不到 `caps`）：
    // 这条断言就是那个等价关系的依据——五级投影里只有 L4 是 `any`。
    for level in PermissionLevel::ALL {
        let is_any = level.caps_view().path_scope == "any";
        assert_eq!(
            is_any,
            level == PermissionLevel::Unrestricted,
            "{level} 的 path_scope"
        );
    }
}

#[test]
fn event_carries_level_axis_and_detail() {
    let event = PermissionAuditEvent {
        level: PermissionLevel::Unrestricted,
        axis: PermissionAxis::Net,
        decision: PermissionDecision::Allowed,
        tool: Some("http_fetch"),
        detail: Some("https://example.com"),
    };
    assert_eq!(event.level.as_str(), "unrestricted");
    assert_eq!(event.axis.as_str(), "net");
    assert_eq!(event.decision.as_str(), "allowed");
    assert_eq!(event.tool, Some("http_fetch"));
    assert_eq!(event.detail, Some("https://example.com"));
}

#[test]
fn emit_does_not_panic_without_subscriber() {
    // tracing 无 subscriber 时是空操作；这里只要证明调用面安全（生产不会因审计崩）。
    record_allowed(
        PermissionLevel::Unrestricted,
        ApprovalPolicy::AuditOnly,
        PermissionAxis::Path,
        Some("read_file"),
        None,
    );
    record_denied(
        PermissionLevel::Workspace,
        ApprovalPolicy::Deny,
        PermissionAxis::Command,
        Some("run_command"),
        Some("git"),
    );
}

/// 判定通过后确实出了一条记录，且五个字段都在。
#[test]
fn allowed_event_reaches_tracing_with_all_fields() {
    let captured = captured_with(|| {
        record_allowed(
            PermissionLevel::Unrestricted,
            ApprovalPolicy::AuditOnly,
            PermissionAxis::Net,
            Some("http_fetch"),
            Some("https://example.com"),
        );
    });
    assert_eq!(
        captured.matches("permission_audit").count(),
        1,
        "{captured}"
    );
    for needle in [
        "permission_level=unrestricted",
        "permission_axis=net",
        "permission_decision=allowed",
        "tool=http_fetch",
        "detail=https://example.com",
    ] {
        assert!(captured.contains(needle), "缺 {needle}，实际：{captured}");
    }
}

#[test]
fn gate_level_allowed_emits_nothing() {
    // L3 的白名单命中不得产出审计记录（否则日志被常态放行淹没）。
    let captured = captured_with(|| {
        record_allowed(
            PermissionLevel::Approved,
            ApprovalPolicy::Gate,
            PermissionAxis::Command,
            Some("run_command"),
            Some("git"),
        );
    });
    assert!(captured.is_empty(), "{captured}");
}

#[test]
fn approved_event_is_recorded_for_any_level() {
    // 人工批准（`Gate`）要留痕：这是「人点了确认」，与等级不设限是两件事。
    let captured = captured_with(|| {
        record_approved(
            PermissionLevel::Approved,
            PermissionAxis::Command,
            None,
            Some("git push"),
        );
    });
    assert!(
        captured.contains("permission_decision=approved"),
        "{captured}"
    );
    assert!(captured.contains("permission_level=approved"), "{captured}");
}

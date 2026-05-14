//! 分层子目标时间线（`hierarchical_subgoal:*`）的**协议行前缀**与界面展示。
//! 服务端当前以中文键为主；前端同时识别英文键以便后续后端双语化。

use super::Locale;

// --- 协议行：`- 键：值` 或 `键：值`（与 `src/agent/hierarchy` 发射文本对齐）---

pub(crate) fn strip_hierarchical_kv<'a>(
    line: &'a str,
    zh: &[&str],
    en: &[&str],
) -> Option<&'a str> {
    let t = line.trim_start();
    for p in zh.iter().chain(en.iter()) {
        if let Some(rest) = t.strip_prefix(p) {
            let v = rest.trim();
            return (!v.is_empty()).then_some(v);
        }
    }
    None
}

pub(crate) fn hierarchical_phase_value_raw(line: &str) -> Option<&str> {
    strip_hierarchical_kv(line, &["- 阶段：", "阶段："], &["- Phase: ", "Phase: "])
}

pub(crate) fn hierarchical_goal_target_raw(line: &str) -> Option<&str> {
    strip_hierarchical_kv(line, &["- 目标：", "目标："], &["- Goal: ", "Goal: "])
}

pub(crate) fn hierarchical_error_count_raw(line: &str) -> Option<&str> {
    strip_hierarchical_kv(
        line,
        &["- 错误数：", "错误数："],
        &["- Error count: ", "Error count: "],
    )
}

pub(crate) fn hierarchical_stagnant_rounds_raw(line: &str) -> Option<&str> {
    strip_hierarchical_kv(
        line,
        &["- 无进展轮次：", "无进展轮次："],
        &["- Stagnant rounds: ", "Stagnant rounds: "],
    )
}

/// 与 [`super::messages::msg_staged_timeline_exec_banner`] 等一致：将协议中的阶段文本规范为内部键。
pub fn hierarchical_subgoal_phase_key(phase: Option<&str>) -> Option<&'static str> {
    let p = phase.map(str::trim).unwrap_or("").to_ascii_lowercase();
    if p.is_empty() {
        return None;
    }
    match p.as_str() {
        "诊断" | "diagnose" => Some("diagnose"),
        "修复" | "fix" => Some("fix"),
        "验证" | "verify" => Some("verify"),
        "升级" | "escalate" => Some("escalate"),
        "开始执行" | "start" | "running" => Some("run"),
        _ => None,
    }
}

pub fn hierarchical_phase_chip_class(key: &str) -> &'static str {
    match key {
        "diagnose" => "msg-subgoal-phase-chip phase-diagnose",
        "fix" => "msg-subgoal-phase-chip phase-fix",
        "verify" => "msg-subgoal-phase-chip phase-verify",
        "escalate" => "msg-subgoal-phase-chip phase-escalate",
        _ => "msg-subgoal-phase-chip",
    }
}

/// 气泡内阶段芯片：已知阶段按界面语言展示短标签；未知则保留原文。
pub fn hierarchical_phase_chip_label(loc: Locale, phase_raw: &str) -> String {
    let phase_raw = phase_raw.trim();
    if phase_raw.is_empty() {
        return String::new();
    }
    let Some(key) = hierarchical_subgoal_phase_key(Some(phase_raw)) else {
        return phase_raw.to_string();
    };
    match loc {
        Locale::ZhHans => match key {
            "diagnose" => "诊断".to_string(),
            "fix" => "修复".to_string(),
            "verify" => "验证".to_string(),
            "escalate" => "升级".to_string(),
            "run" => "开始执行".to_string(),
            _ => phase_raw.to_string(),
        },
        Locale::En => match key {
            "diagnose" => "Diagnose".to_string(),
            "fix" => "Fix".to_string(),
            "verify" => "Verify".to_string(),
            "escalate" => "Escalate".to_string(),
            "run" => "Running".to_string(),
            _ => phase_raw.to_string(),
        },
    }
}

/// 子目标气泡正文首条「阶段」行的原始值（中英键均可）。
pub fn hierarchical_phase_raw_from_body(text: &str) -> Option<String> {
    let v = text
        .lines()
        .map(str::trim)
        .find_map(hierarchical_phase_value_raw)?;
    let t = v.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// 阶段芯片：`(界面语言标签, CSS class 字符串)`。
pub fn hierarchical_phase_chip_view(loc: Locale, msg_text: &str) -> Option<(String, String)> {
    let raw = hierarchical_phase_raw_from_body(msg_text)?;
    let key = hierarchical_subgoal_phase_key(Some(raw.as_str())).unwrap_or("");
    let label = hierarchical_phase_chip_label(loc, raw.as_str());
    let cls = hierarchical_phase_chip_class(key).to_string();
    Some((label, cls))
}

pub fn hierarchical_metrics_line(
    loc: Locale,
    error_count: Option<&str>,
    stagnant: Option<&str>,
) -> Option<String> {
    if error_count.is_none() && stagnant.is_none() {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(v) = error_count.filter(|s| !s.trim().is_empty()) {
        parts.push(match loc {
            Locale::ZhHans => format!("错误数 {}", v.trim()),
            Locale::En => format!("Error count {}", v.trim()),
        });
    }
    if let Some(v) = stagnant.filter(|s| !s.trim().is_empty()) {
        parts.push(match loc {
            Locale::ZhHans => format!("无进展 {} 轮", v.trim()),
            Locale::En => format!("{} stagnant rounds", v.trim()),
        });
    }
    Some(parts.join(" · "))
}

pub fn hierarchical_subgoal_exec_verb(loc: Locale, key: &str) -> &'static str {
    match (loc, key) {
        (Locale::ZhHans, "diagnose") => "正在诊断",
        (Locale::ZhHans, "fix") => "正在修复",
        (Locale::ZhHans, "verify") => "正在验证",
        (Locale::ZhHans, "escalate") => "正在升级",
        (Locale::ZhHans, "run") => "正在执行",
        (Locale::En, "diagnose") => "Diagnosing",
        (Locale::En, "fix") => "Fixing",
        (Locale::En, "verify") => "Verifying",
        (Locale::En, "escalate") => "Escalating",
        (Locale::En, "run") => "Running",
        _ => "",
    }
}

pub fn hierarchical_subgoal_running_suffix(loc: Locale) -> &'static str {
    match loc {
        Locale::ZhHans => "子目标…",
        Locale::En => "subgoal…",
    }
}

pub fn hierarchical_subgoal_title_prefixes() -> [&'static str; 2] {
    ["子目标 `", "Subgoal `"]
}

pub fn hierarchical_subgoal_title_second_line_prefixes() -> [&'static str; 2] {
    ["子目标 ", "Subgoal "]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_phase_goal_en_zh() {
        let zh = "- 阶段：开始执行";
        let en = "- Phase: running";
        assert_eq!(hierarchical_phase_value_raw(zh), Some("开始执行"));
        assert_eq!(hierarchical_phase_value_raw(en), Some("running"));
        assert_eq!(
            hierarchical_goal_target_raw("- Goal: build dir"),
            Some("build dir")
        );
        assert_eq!(
            hierarchical_goal_target_raw("- 目标：创建 build 目录"),
            Some("创建 build 目录")
        );
    }

    #[test]
    fn metrics_line_en() {
        let s = hierarchical_metrics_line(Locale::En, Some("2"), Some("3")).unwrap();
        assert!(s.contains("Error count 2"));
        assert!(s.contains("3 stagnant rounds"));
    }
}

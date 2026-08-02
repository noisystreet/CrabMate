//! 会话工作模式（Ask / Plan / Act），与 `agent_role` 人格正交。

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// 用户显式控制的会话能力档：决定工具收窄与 system 模式附录。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionMode {
    /// 只读：解释 / 问答，不改仓库、不跑构建测试。
    Ask,
    /// 只读规划：可调研并产出方案，不写盘。
    Plan,
    /// 执行：全量工具（再与角色 `allowed_tools` 取交）。
    #[default]
    Act,
}

impl SessionMode {
    pub const ALL: [SessionMode; 3] = [SessionMode::Ask, SessionMode::Plan, SessionMode::Act];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SessionMode::Ask => "ask",
            SessionMode::Plan => "plan",
            SessionMode::Act => "act",
        }
    }

    /// Ask / Plan 应对本回合施加只读工具策略。
    #[must_use]
    pub fn requires_readonly_tools(self) -> bool {
        matches!(self, SessionMode::Ask | SessionMode::Plan)
    }

    #[must_use]
    pub fn display_title_zh(self) -> &'static str {
        match self {
            SessionMode::Ask => "Ask（只读）",
            SessionMode::Plan => "Plan（只读规划）",
            SessionMode::Act => "Act（执行）",
        }
    }
}

impl fmt::Display for SessionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SessionMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_session_mode(s)
    }
}

/// 解析 `ask` / `plan` / `act`（大小写不敏感）；空串或未知 → `Err`。
pub fn parse_session_mode(raw: &str) -> Result<SessionMode, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "ask" => Ok(SessionMode::Ask),
        "plan" => Ok(SessionMode::Plan),
        "act" => Ok(SessionMode::Act),
        "" => Err("session_mode 不能为空（须为 ask / plan / act）".into()),
        other => Err(format!(
            "未知 session_mode \"{other}\"（须为 ask / plan / act）"
        )),
    }
}

/// 可选字段：空 / 缺省 → `None`；非空须合法。
pub fn parse_optional_session_mode(raw: Option<&str>) -> Result<Option<SessionMode>, String> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => parse_session_mode(s).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_case_insensitive() {
        assert_eq!(parse_session_mode("ASK").unwrap(), SessionMode::Ask);
        assert_eq!(parse_session_mode("Plan").unwrap(), SessionMode::Plan);
        assert_eq!(parse_session_mode("act").unwrap(), SessionMode::Act);
    }

    #[test]
    fn readonly_flags() {
        assert!(SessionMode::Ask.requires_readonly_tools());
        assert!(SessionMode::Plan.requires_readonly_tools());
        assert!(!SessionMode::Act.requires_readonly_tools());
    }
}

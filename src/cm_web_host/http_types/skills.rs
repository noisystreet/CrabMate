//! `GET /skills` JSON 体。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SkillListItem {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub description: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillsListResponse {
    pub enabled: bool,
    pub skills_dir: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub skills_user_dir: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub skills_system_dir: String,
    pub skills: Vec<SkillListItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

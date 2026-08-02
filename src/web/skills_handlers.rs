//! `GET /skills`：当前工作区 skills 目录的 JSON 目录（供 Web composer `/` 浮层）。

use axum::Json;
use axum::extract::State;

use super::app_state::AppStateHttpCore;
use super::http_types::skills::{SkillListItem, SkillsListResponse};
use crate::context_bootstrap::prompt_compose::resolve_skills_base_dir;

pub async fn skills_list_handler(State(http): State<AppStateHttpCore>) -> Json<SkillsListResponse> {
    let cfg = http.cfg.read().await;
    let enabled = cfg.skills.skills_enabled;
    let skills_dir = cfg.skills.skills_dir.clone();
    let skills_user_dir = cfg.skills.skills_user_dir.clone();
    let skills_system_dir = cfg.skills.skills_system_dir.clone();
    let list_opts_dirs = (
        skills_dir.clone(),
        skills_user_dir.clone(),
        skills_system_dir.clone(),
    );
    drop(cfg);

    if !enabled {
        return Json(SkillsListResponse {
            enabled: false,
            skills_dir,
            skills_user_dir,
            skills_system_dir,
            skills: Vec::new(),
            error: None,
        });
    }

    let ws = std::path::PathBuf::from(http.effective_workspace_path().await);
    let base_dir = resolve_skills_base_dir(ws.as_path());
    let opts = crate::config::skills::SkillsListOpts {
        workspace_base_dir: base_dir.as_path(),
        skills_dir: list_opts_dirs.0.as_str(),
        skills_user_dir: list_opts_dirs.1.as_str(),
        skills_system_dir: list_opts_dirs.2.as_str(),
    };
    match crate::config::skills_slash::list_skill_catalog_entries(opts) {
        Ok(entries) => Json(SkillsListResponse {
            enabled: true,
            skills_dir,
            skills_user_dir,
            skills_system_dir,
            skills: entries
                .into_iter()
                .map(|e| SkillListItem {
                    id: e.id,
                    name: e.name,
                    description: e.description,
                    path: e.path,
                })
                .collect(),
            error: None,
        }),
        Err(e) => Json(SkillsListResponse {
            enabled: true,
            skills_dir,
            skills_user_dir,
            skills_system_dir,
            skills: Vec::new(),
            error: Some(sanitize_skills_list_error(&e)),
        }),
    }
}

fn sanitize_skills_list_error(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut quoted = String::new();
            while let Some(&q) = chars.peek() {
                chars.next();
                if q == '"' {
                    break;
                }
                quoted.push(q);
            }
            if quoted.starts_with('/') || quoted.starts_with('\\') {
                out.push_str("(path)");
            } else {
                out.push('"');
                out.push_str(&quoted);
                out.push('"');
            }
        } else {
            out.push(c);
        }
    }
    out
}

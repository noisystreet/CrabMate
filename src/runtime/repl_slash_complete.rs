//! REPL `/` 行内补全逻辑（由 [`super::repl_reedline::ReplSlashCompleter`] 调用）。

use reedline::{Span, Suggestion};

/// 内建 `/` 命令名（不含斜杠；`?` 单独成项）。
pub(super) const SLASH_COMMANDS: &[&str] = &[
    "?",
    "agent",
    "api-base",
    "api-key",
    "apikey",
    "apibase",
    "cd",
    "clear",
    "config",
    "doctor",
    "export",
    "help",
    "mcp",
    "model",
    "models",
    "probe",
    "save-session",
    "tools",
    "version",
    "workspace",
];

/// `/export` 与 `/save-session` 后的格式参数（与 REPL `repl_export_kind_from_arg` 一致）。
pub(super) const EXPORT_FORMAT_ARGS: &[&str] = &["both", "json", "markdown", "md"];

pub(super) fn suggestion_slash_command(span: Span, cmd: &str) -> Suggestion {
    let value = if cmd == "?" {
        "/?".to_string()
    } else {
        format!("/{cmd}")
    };
    Suggestion {
        value,
        span,
        append_whitespace: false,
        ..Default::default()
    }
}

fn suggestions_first_token(partial: &str) -> Vec<&'static str> {
    let p = partial.to_ascii_lowercase();
    let mut hits: Vec<&str> = SLASH_COMMANDS
        .iter()
        .copied()
        .filter(|c| p.is_empty() || c.to_ascii_lowercase().starts_with(&p))
        .collect();
    hits.sort_unstable();
    hits.dedup();
    hits
}

fn suggestions_session_export_formats(span: Span, prefix: &str) -> Vec<Suggestion> {
    EXPORT_FORMAT_ARGS
        .iter()
        .copied()
        .map(|a| Suggestion {
            value: format!("{prefix} {a}"),
            span,
            append_whitespace: false,
            ..Default::default()
        })
        .collect()
}

/// `tail` = `line[slash_idx+1..pos]`，且尚未出现空白（即仍在「第一个 token」上）。
pub(super) fn complete_slash_no_whitespace_tail(span: Span, tail: &str) -> Vec<Suggestion> {
    if tail.eq_ignore_ascii_case("export") {
        return suggestions_session_export_formats(span, "/export");
    }
    if tail.eq_ignore_ascii_case("save-session") {
        return suggestions_session_export_formats(span, "/save-session");
    }
    if tail.eq_ignore_ascii_case("config") {
        return ["reload"]
            .iter()
            .map(|a| Suggestion {
                value: format!("/config {a}"),
                span,
                append_whitespace: false,
                ..Default::default()
            })
            .collect();
    }
    if tail.eq_ignore_ascii_case("mcp") {
        return ["list", "probe"]
            .iter()
            .map(|a| Suggestion {
                value: format!("/mcp {a}"),
                span,
                append_whitespace: false,
                ..Default::default()
            })
            .collect();
    }
    if tail.eq_ignore_ascii_case("models") {
        return ["list", "choose"]
            .iter()
            .map(|a| {
                let value = if *a == "choose" {
                    format!("/models {a} ")
                } else {
                    format!("/models {a}")
                };
                Suggestion {
                    value,
                    span,
                    append_whitespace: false,
                    ..Default::default()
                }
            })
            .collect();
    }
    if tail.eq_ignore_ascii_case("model") {
        return ["set"]
            .iter()
            .map(|a| Suggestion {
                value: format!("/model {a} "),
                span,
                append_whitespace: false,
                ..Default::default()
            })
            .collect();
    }
    if tail.eq_ignore_ascii_case("api-base") || tail.eq_ignore_ascii_case("apibase") {
        let p = if tail.eq_ignore_ascii_case("apibase") {
            "/apibase"
        } else {
            "/api-base"
        };
        return ["set"]
            .iter()
            .map(|a| Suggestion {
                value: format!("{p} {a} "),
                span,
                append_whitespace: false,
                ..Default::default()
            })
            .collect();
    }
    if tail.eq_ignore_ascii_case("agent") {
        return ["list", "set"]
            .iter()
            .map(|a| {
                let value = if *a == "set" {
                    format!("/agent {a} ")
                } else {
                    format!("/agent {a}")
                };
                Suggestion {
                    value,
                    span,
                    append_whitespace: false,
                    ..Default::default()
                }
            })
            .collect();
    }
    suggestions_first_token(tail)
        .into_iter()
        .map(|cmd| suggestion_slash_command(span, cmd))
        .collect()
}

fn complete_slash_config_second(span: Span, after_ws: &str) -> Vec<Suggestion> {
    let ap = after_ws.trim_start();
    let ap_l = ap.to_ascii_lowercase();
    let hits: Vec<&str> = if ap_l.is_empty() || "reload".starts_with(ap_l.as_str()) {
        vec!["reload"]
    } else {
        vec![]
    };
    hits.into_iter()
        .map(|a| Suggestion {
            value: format!("/config {a}"),
            span,
            append_whitespace: false,
            ..Default::default()
        })
        .collect()
}

fn complete_slash_mcp_second(span: Span, after_ws: &str) -> Vec<Suggestion> {
    let ap = after_ws.trim_start();
    let ap_l = ap.to_ascii_lowercase();
    let hits: Vec<&str> = if ap_l.is_empty() {
        vec!["list", "probe"]
    } else if ap_l.starts_with("list") {
        let rest = ap_l.strip_prefix("list").unwrap_or("").trim_start();
        if rest.is_empty() {
            vec!["list", "list probe"]
        } else if "probe".starts_with(rest) {
            vec!["list probe"]
        } else {
            vec![]
        }
    } else {
        ["list", "probe"]
            .iter()
            .copied()
            .filter(|s| s.starts_with(ap_l.as_str()))
            .collect()
    };
    hits.into_iter()
        .map(|a| Suggestion {
            value: format!("/mcp {a}"),
            span,
            append_whitespace: false,
            ..Default::default()
        })
        .collect()
}

fn complete_slash_models_second(span: Span, after_ws: &str) -> Vec<Suggestion> {
    let ap = after_ws.trim_start();
    let ap_l = ap.to_ascii_lowercase();
    let hits: Vec<&str> = if ap_l.is_empty() {
        vec!["list", "choose"]
    } else {
        ["list", "choose"]
            .iter()
            .copied()
            .filter(|s| s.starts_with(ap_l.as_str()))
            .collect()
    };
    hits.into_iter()
        .map(|a| {
            let value = if a == "choose" {
                format!("/models {a} ")
            } else {
                format!("/models {a}")
            };
            Suggestion {
                value,
                span,
                append_whitespace: false,
                ..Default::default()
            }
        })
        .collect()
}

fn complete_slash_agent_second(span: Span, after_ws: &str) -> Vec<Suggestion> {
    let ap = after_ws.trim_start();
    let ap_l = ap.to_ascii_lowercase();
    let hits: Vec<&str> = if ap_l.is_empty() {
        vec!["list", "set"]
    } else {
        ["list", "set"]
            .iter()
            .copied()
            .filter(|s| s.starts_with(ap_l.as_str()))
            .collect()
    };
    hits.into_iter()
        .map(|a| {
            let value = if a == "set" {
                format!("/agent {a} ")
            } else {
                format!("/agent {a}")
            };
            Suggestion {
                value,
                span,
                append_whitespace: false,
                ..Default::default()
            }
        })
        .collect()
}

fn complete_slash_model_second(span: Span, after_ws: &str) -> Vec<Suggestion> {
    let ap = after_ws.trim_start();
    let ap_l = ap.to_ascii_lowercase();
    let hits: Vec<&str> = if ap_l.is_empty() {
        vec!["set"]
    } else {
        ["set"]
            .iter()
            .copied()
            .filter(|s| s.starts_with(ap_l.as_str()))
            .collect()
    };
    hits.into_iter()
        .map(|a| {
            let value = if a == "set" {
                format!("/model {a} ")
            } else {
                format!("/model {a}")
            };
            Suggestion {
                value,
                span,
                append_whitespace: false,
                ..Default::default()
            }
        })
        .collect()
}

fn complete_slash_api_base_second(span: Span, cmd: &str, after_ws: &str) -> Vec<Suggestion> {
    let slash = if cmd.eq_ignore_ascii_case("apibase") {
        "/apibase"
    } else {
        "/api-base"
    };
    let ap = after_ws.trim_start();
    let ap_l = ap.to_ascii_lowercase();
    let hits: Vec<&str> = if ap_l.is_empty() {
        vec!["set"]
    } else {
        ["set"]
            .iter()
            .copied()
            .filter(|s| s.starts_with(ap_l.as_str()))
            .collect()
    };
    hits.into_iter()
        .map(|a| {
            let value = if a == "set" {
                format!("{slash} {a} ")
            } else {
                format!("{slash} {a}")
            };
            Suggestion {
                value,
                span,
                append_whitespace: false,
                ..Default::default()
            }
        })
        .collect()
}

/// `cmd` = 第一个 token（已 trim），`after_ws` = 其后的原文（可含前导空白）。
pub(super) fn complete_slash_after_whitespace(
    span: Span,
    cmd: &str,
    after_ws: &str,
) -> Vec<Suggestion> {
    if cmd.eq_ignore_ascii_case("config") {
        return complete_slash_config_second(span, after_ws);
    }
    if cmd.eq_ignore_ascii_case("mcp") {
        return complete_slash_mcp_second(span, after_ws);
    }
    if cmd.eq_ignore_ascii_case("models") {
        return complete_slash_models_second(span, after_ws);
    }
    if cmd.eq_ignore_ascii_case("agent") {
        return complete_slash_agent_second(span, after_ws);
    }
    if cmd.eq_ignore_ascii_case("model") {
        return complete_slash_model_second(span, after_ws);
    }
    if cmd.eq_ignore_ascii_case("api-base") || cmd.eq_ignore_ascii_case("apibase") {
        return complete_slash_api_base_second(span, cmd, after_ws);
    }
    let prefix = if cmd.eq_ignore_ascii_case("export") {
        "/export"
    } else if cmd.eq_ignore_ascii_case("save-session") {
        "/save-session"
    } else {
        return vec![];
    };
    let arg_prefix = after_ws.trim_start();
    let p = arg_prefix.to_ascii_lowercase();
    EXPORT_FORMAT_ARGS
        .iter()
        .copied()
        .filter(|a| p.is_empty() || a.to_ascii_lowercase().starts_with(&p))
        .map(|a| Suggestion {
            value: format!("{prefix} {a}"),
            span,
            append_whitespace: false,
            ..Default::default()
        })
        .collect()
}

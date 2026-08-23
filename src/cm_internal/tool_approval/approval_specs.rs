//! 各敏感工具的 [`ApprovalRequestSpec`] 构建器，避免在分发层重复拼装字段。

use super::{ApprovalRequestSpec, SensitiveCapability};

const HTTP_PREFIX_MISS_DETAIL: &str =
    "URL 未匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）：\n";

/// `http_fetch`：出站只读 HTTP，未匹配配置前缀时需人工审批。
pub fn http_fetch(approval_args: &str, allowlist_key: &str) -> ApprovalRequestSpec {
    ApprovalRequestSpec {
        capability: SensitiveCapability::OutboundHttpRead,
        sse_command: "http_fetch".to_string(),
        sse_args: approval_args.to_string(),
        allowlist_key: Some(allowlist_key.to_string()),
        cli_title: "http_fetch 审批",
        cli_detail: format!("{HTTP_PREFIX_MISS_DETAIL}{approval_args}"),
        web_timeline_prefix_zh: "http_fetch 审批：",
    }
}

/// `http_request`：出站可写/非常规方法 HTTP，未匹配配置前缀时需人工审批。
pub fn http_request(approval_args: &str, allowlist_key: &str) -> ApprovalRequestSpec {
    ApprovalRequestSpec {
        capability: SensitiveCapability::OutboundHttpWrite,
        sse_command: "http_request".to_string(),
        sse_args: approval_args.to_string(),
        allowlist_key: Some(allowlist_key.to_string()),
        cli_title: "http_request 审批",
        cli_detail: format!("{HTTP_PREFIX_MISS_DETAIL}{approval_args}"),
        web_timeline_prefix_zh: "http_request 审批：",
    }
}

/// `read_dir` 访问工作区外路径（绝对路径或 `..`）。
pub fn read_dir_external_path(ext_path: &str) -> ApprovalRequestSpec {
    ApprovalRequestSpec {
        capability: SensitiveCapability::WorkspaceExternalPath,
        sse_command: "read_dir".to_string(),
        sse_args: format!("path={ext_path}"),
        allowlist_key: None,
        cli_title: "read_dir 工作区外路径审批",
        cli_detail: format!(
            "read_dir 请求访问工作区外路径：{ext_path}\n仅在可信环境下批准。"
        ),
        web_timeline_prefix_zh: "工作区外路径审批：",
    }
}

/// `run_command`：单命令不在白名单。
pub fn run_command_unknown_cmd(cmd: &str, cmd_show: &str) -> ApprovalRequestSpec {
    ApprovalRequestSpec {
        capability: SensitiveCapability::HostShell,
        sse_command: cmd.to_string(),
        sse_args: cmd_show.to_string(),
        allowlist_key: None,
        cli_title: "run_command 审批",
        cli_detail: format!("命令不在白名单；审批对象为完整脚本:\n{}", cmd_show.trim()),
        web_timeline_prefix_zh: "命令审批：",
    }
}

/// `run_command` / `terminal_session`：需经 `bash -c` 执行的整行脚本。
pub fn shell_script(sse_command: &str, script: &str) -> ApprovalRequestSpec {
    let cli_title = if sse_command == "terminal_session" {
        "terminal_session 脚本审批"
    } else {
        "run_command 脚本审批"
    };
    ApprovalRequestSpec {
        capability: SensitiveCapability::HostShell,
        sse_command: sse_command.to_string(),
        sse_args: script.to_string(),
        allowlist_key: None,
        cli_title,
        cli_detail: format!(
            "将经 bash -c 执行整行（glob / $VAR 会展开；独立 argv 中的 && / | 等会绕过单命令白名单）：\n{}",
            script.trim()
        ),
        web_timeline_prefix_zh: "脚本审批：",
    }
}

/// `run_command` / `terminal_session`：参数含工作区外路径或 `..`。
pub fn workspace_external_path(sse_command: &str, detail_paths: &str) -> ApprovalRequestSpec {
    let cli_title = if sse_command == "terminal_session" {
        "terminal_session 工作区外路径审批"
    } else {
        "run_command 工作区外路径审批"
    };
    ApprovalRequestSpec {
        capability: SensitiveCapability::WorkspaceExternalPath,
        sse_command: sse_command.to_string(),
        sse_args: format!("external_paths={detail_paths}"),
        allowlist_key: None,
        cli_title,
        cli_detail: format!(
            "{sse_command} 请求使用工作区外路径或 \"..\"：{detail_paths}\n仅在可信环境下批准。\n（不审计 bash/sh -c 脚本字符串内部路径。）"
        ),
        web_timeline_prefix_zh: "工作区外路径审批：",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_approval::SensitiveCapability;

    #[test]
    fn http_fetch_spec_fields() {
        let spec = http_fetch("GET https://ex.com/path", "http_fetch:https://ex.com/path");
        assert_eq!(spec.capability, SensitiveCapability::OutboundHttpRead);
        assert_eq!(spec.sse_command, "http_fetch");
        assert_eq!(spec.sse_args, "GET https://ex.com/path");
        assert_eq!(
            spec.allowlist_key.as_deref(),
            Some("http_fetch:https://ex.com/path")
        );
        assert!(spec.cli_detail.contains("http_fetch_allowed_prefixes"));
    }

    #[test]
    fn run_command_unknown_cmd_sse_command_is_argv0() {
        let spec = run_command_unknown_cmd("git", "git status");
        assert_eq!(spec.capability, SensitiveCapability::HostShell);
        assert_eq!(spec.sse_command, "git");
        assert_eq!(spec.sse_args, "git status");
        assert_eq!(spec.allowlist_key, None);
    }
}

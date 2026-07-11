//! GitHub 在线模式侧栏文案。

use super::Locale;

pub fn github_panel_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Pull Requests",
        Locale::En => "Pull Requests",
    }
}

pub fn github_loading(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载中…",
        Locale::En => "Loading…",
    }
}

pub fn github_error(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载失败",
        Locale::En => "Load failed",
    }
}

pub fn github_refresh(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "刷新",
        Locale::En => "Refresh",
    }
}

pub fn github_loading_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "正在加载 GitHub 数据",
        Locale::En => "Loading GitHub data",
    }
}

pub fn github_not_connected(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "当前工作区未关联 GitHub 仓库，或本机未安装/授权 gh CLI。",
        Locale::En => {
            "Workspace is not linked to a GitHub repo, or gh CLI is missing or not authenticated."
        }
    }
}

pub fn github_repo_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "仓库",
        Locale::En => "Repo",
    }
}

pub fn github_branch_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "分支",
        Locale::En => "Branch",
    }
}

pub fn github_open_prs(l: Locale, count: usize) -> String {
    match l {
        Locale::ZhHans => format!("{count} 个 open PR"),
        Locale::En => format!("{count} open PR(s)"),
    }
}

pub fn github_checks_summary(l: Locale, passing: usize, failing: usize, pending: usize) -> String {
    match l {
        Locale::ZhHans => format!("✓{passing} ✗{failing} …{pending}"),
        Locale::En => format!("✓{passing} ✗{failing} …{pending}"),
    }
}

pub fn github_status_chip_pr(l: Locale, number: u64, title: &str) -> String {
    let short = if title.chars().count() > 28 {
        format!("{}…", title.chars().take(28).collect::<String>())
    } else {
        title.to_string()
    };
    match l {
        Locale::ZhHans => format!("#{number} {short}"),
        Locale::En => format!("#{number} {short}"),
    }
}

pub fn github_status_chip_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "PR",
        Locale::En => "PR",
    }
}

pub fn github_side_view_menu(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Pull Requests",
        Locale::En => "Pull Requests",
    }
}

pub fn github_no_open_prs(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "暂无 open PR",
        Locale::En => "No open pull requests",
    }
}

pub fn github_checks_heading(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "当前分支 CI",
        Locale::En => "Current branch CI",
    }
}

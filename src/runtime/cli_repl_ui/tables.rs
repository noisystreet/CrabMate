//! `/help` 表格数据与说明列软换行。

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// 左缘空白列数（`"  "`）。
pub(super) const HELP_LEFT: usize = 2;
/// 命令列与说明之间的空隙列数。
pub(super) const HELP_GAP: usize = 1;
/// 表格布局时第一行至少留给说明的列数；不足则改为「命令单独一行」。
pub(super) const HELP_DESC_MIN: usize = 8;

/// `/help` 命令列与说明（与 [`super::CliReplStyle::print_help`] 同源）。
pub(super) const REPL_HELP_ROWS: &[(&str, &str)] = &[
    ("/clear", "清空对话，仅保留当前 system 提示词"),
    (
        "/model · /model set <名称>",
        "显示或写入内存中的 model（set 不校验 GET /models 列表；持久化请改配置）",
    ),
    (
        "/api-base · /api-base set <url> · /apibase …",
        "显示或写入内存中的 api_base（OpenAI 兼容网关根；持久化请改配置）",
    ),
    (
        "/api-key · /api-key status · /api-key set <密钥> · /api-key clear",
        "本进程内存中的 LLM Bearer 密钥（不写盘；未 export API_KEY 时可用；/config reload 不清除）",
    ),
    (
        "/config",
        "打印关键运行配置摘要（与启动横幅同源字段；不含密钥）",
    ),
    (
        "/config reload",
        "从磁盘+环境变量热重载可更字段（不含会话 SQLite 路径；详见文档）",
    ),
    (
        "/doctor",
        "一页环境诊断（同 crabmate doctor；不要求 API_KEY）",
    ),
    (
        "/probe",
        "探测 api_base 的 GET …/models 连通性（同 crabmate probe；需 bearer 时依赖 API_KEY）",
    ),
    (
        "/models · /models list",
        "列出 GET …/models 返回的模型 id（同 crabmate models；需 bearer 时依赖 API_KEY）",
    ),
    (
        "/models choose <id>",
        "从上述列表设当前 model（内存；支持唯一前缀；持久化请改配置）",
    ),
    (
        "/agent · /agent list",
        "列出内建 default 与配置中的命名角色 id（与 REPL「当前」行一致；无表时提示未启用多角色）",
    ),
    (
        "/agent set <id> | /agent set default",
        "set <id>：须存在于角色表；**set default**：清除显式角色，与 Web 未选角色及「默认」逻辑一致（default_agent_role_id 或全局 system）",
    ),
    ("/workspace", "显示当前工作区"),
    (
        "/workspace <路径>",
        "切换工作区（须为已存在目录，别名 /cd）：相对路径同 read_file（相对当前根、禁止 / 开头）；绝对路径须落在 workspace_allowed_roots",
    ),
    (
        "/skills · /skills list",
        "列出当前工作区下可见的 skills 文件",
    ),
    ("/tools", "列出当前加载的工具名"),
    (
        "/export [json|markdown|both]",
        "导出当前内存对话到 .crabmate/exports/（与 Web 同形 JSON/Markdown）",
    ),
    (
        "/save-session [json|markdown|both]",
        "从磁盘会话文件导出到 .crabmate/exports/（同 crabmate save-session；默认 tui_session.json）",
    ),
    (
        "/mcp · /mcp list · /mcp probe · /mcp list probe",
        "列出本进程内 MCP stdio 缓存与合并工具名（同 crabmate mcp list；probe 会启动 mcp_command 子进程）",
    ),
    ("/version", "打印 crabmate 版本与 OS/ARCH（不含密钥）"),
    ("/help, /?", "本说明"),
    (
        "$ → bash#:",
        "交互终端行首按 `$` 后提示变为 bash#: 并输入命令；管道输入仍可用 `$ <命令>`",
    ),
];

pub(super) fn pad_cmd_to_display_width(cmd: &str, target: usize) -> String {
    let mut s = cmd.to_string();
    while s.width() < target {
        s.push(' ');
    }
    s
}

pub(super) fn spaces_to_display_width(target: usize) -> String {
    let mut s = String::new();
    while s.width() < target {
        s.push(' ');
    }
    s
}

/// 无空格且超过 `max_w` 显示宽度的片段，按字符边界硬拆行。
fn break_long_word(word: &str, max_w: usize) -> Vec<String> {
    let max_w = max_w.max(1);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut acc = 0usize;
    for ch in word.chars() {
        let cw = ch.width().unwrap_or(0).max(1);
        if acc + cw > max_w {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                acc = 0;
            }
            if cw > max_w {
                out.push(ch.to_string());
                continue;
            }
        }
        cur.push(ch);
        acc += cw;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 按空白分词软换行；显示宽度用 [`UnicodeWidthStr`]（CJK 等按终端惯例计宽）。
pub(super) fn wrap_help_description(text: &str, max_w: usize) -> Vec<String> {
    let max_w = max_w.max(1);
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut cur_w = 0usize;

    for word in text.split_whitespace() {
        let ww = word.width();
        let need_space = !current.is_empty();
        let sep_w = need_space as usize;

        if ww > max_w {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                cur_w = 0;
            }
            for chunk in break_long_word(word, max_w) {
                lines.push(chunk);
            }
            continue;
        }

        if cur_w + sep_w + ww > max_w {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            current.push_str(word);
            cur_w = ww;
        } else {
            if need_space {
                current.push(' ');
                cur_w += 1;
            }
            current.push_str(word);
            cur_w += ww;
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

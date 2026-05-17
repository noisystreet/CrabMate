//! 从 Markdown 提取 `` ```crabmate-workflow `` 围栏块。

const FENCE_INFO: &str = "crabmate-workflow";

/// 返回文档内全部 `crabmate-workflow` 代码块正文（不含围栏行）。
pub(crate) fn extract_crabmate_workflow_blocks(md: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut buf = String::new();

    for line in md.lines() {
        let trimmed = line.trim();
        if !in_block {
            if let Some(rest) = trimmed.strip_prefix("```") {
                let info = rest.trim();
                if info == FENCE_INFO {
                    in_block = true;
                    buf.clear();
                }
            }
            continue;
        }
        if trimmed.starts_with("```") {
            blocks.push(buf.trim_end().to_string());
            in_block = false;
            buf.clear();
            continue;
        }
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(line);
    }

    blocks
}

/// 取首个块；无块则 `Err`。
pub(crate) fn extract_first_crabmate_workflow_block(md: &str) -> Result<String, String> {
    let blocks = extract_crabmate_workflow_blocks(md);
    blocks
        .into_iter()
        .next()
        .ok_or_else(|| format!("Markdown 中未找到 ```{FENCE_INFO} 围栏块"))
}

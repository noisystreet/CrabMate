//! 产物提取：从 agent 执行结果中提取 benchmark 所需的输出产物。

use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

/// 从工作区执行 `git diff HEAD` 提取 unified diff patch。
pub fn extract_git_patch(work_dir: &Path) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(work_dir)
        .output()
        .map_err(|e| format!("执行 git diff 失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff 返回非零: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 从 agent 回复中提取 GAIA 格式的最终答案。
pub fn extract_final_answer(reply: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)FINAL\s+ANSWER\s*:\s*(.+?)(?:\n|$)").expect("FINAL ANSWER regex invalid")
    });

    RE.captures_iter(reply)
        .last()
        .map(|cap| cap[1].trim().to_string())
}

/// 从 agent 回复中提取代码补全（围栏内正文，否则整段回复）。
pub fn extract_code_completion(reply: &str) -> String {
    static CODE_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)```(?:python|py)?\s*\n(.*?)```").expect("code block regex invalid")
    });

    if let Some(cap) = CODE_BLOCK.captures(reply) {
        return cap[1].to_string();
    }

    reply.to_string()
}

/// HumanEval 官方 `check` 会执行 `prompt + completion`。若模型复述了函数桩，剥掉前缀以免重复 `def`。
pub fn extract_humaneval_completion(reply: &str, prompt: &str) -> String {
    strip_repeated_prompt_prefix(&extract_code_completion(reply), prompt)
}

fn strip_repeated_prompt_prefix(completion: &str, prompt: &str) -> String {
    if prompt.is_empty() {
        return completion.to_string();
    }
    let body = completion.trim_start_matches(['\n', '\r']);
    for prefix in [prompt, prompt.trim_end()] {
        if prefix.is_empty() {
            continue;
        }
        if let Some(rest) = body.strip_prefix(prefix) {
            return rest.trim_start_matches('\n').to_string();
        }
    }
    completion.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_final_answer() {
        let reply = "经过分析，答案如下：\nFINAL ANSWER: 42\n其他文本";
        assert_eq!(extract_final_answer(reply), Some("42".to_string()));
    }

    #[test]
    fn test_extract_final_answer_multiple() {
        let reply = "FINAL ANSWER: wrong\n改正：\nFINAL ANSWER: correct";
        assert_eq!(extract_final_answer(reply), Some("correct".to_string()));
    }

    #[test]
    fn test_extract_final_answer_none() {
        assert_eq!(extract_final_answer("没有答案标记"), None);
    }

    #[test]
    fn test_extract_code_completion_fenced() {
        let reply = "说明文本\n```python\ndef add(a, b):\n    return a + b\n```\n后续";
        let code = extract_code_completion(reply);
        assert!(code.contains("def add(a, b):"));
        assert!(!code.contains("说明文本"));
    }

    #[test]
    fn test_extract_code_completion_no_fence() {
        let reply = "def add(a, b):\n    return a + b\n";
        let code = extract_code_completion(reply);
        assert!(code.contains("def add"));
    }

    #[test]
    fn test_extract_humaneval_strips_repeated_prompt() {
        let prompt = "def add(a, b):\n    \"\"\"Add two numbers.\"\"\"\n";
        let reply =
            "```python\ndef add(a, b):\n    \"\"\"Add two numbers.\"\"\"\n    return a + b\n```";
        let code = extract_humaneval_completion(reply, prompt);
        assert_eq!(code, "    return a + b\n");
        assert!(!code.contains("def add"));
    }

    #[test]
    fn test_extract_humaneval_keeps_body_only() {
        let prompt = "def add(a, b):\n    \"\"\"Add two numbers.\"\"\"\n";
        let reply = "    return a + b\n";
        let code = extract_humaneval_completion(reply, prompt);
        assert_eq!(code, "    return a + b\n");
    }
}

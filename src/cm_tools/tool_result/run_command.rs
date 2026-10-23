//! `run_command` 输出解析：命令回显、退出码与 stdout/stderr 分流。
//!
//! 输出形状为 `命令：{argv}⏎退出码：{N}⏎标准输出：⏎…⏎标准错误：⏎…`。但 **`命令：`
//! 回显会把原始 argv 原样贴出**——多行参数（如 `git commit -m "多行正文"`）会让该行
//! 跨越数十行。因此「剥掉首行后首行即状态行」是不成立的假设，取退出码须用
//! [`exit_code_in_header_region`]（在首个流标记行之前逐行找）。

use serde_json::Value;

/// 分隔 stdout / stderr 的固定标记行，由 `run_command` 输出格式化端写入。
const STREAM_MARKERS: [&str; 2] = ["标准输出：", "标准错误："];
const STDOUT_MARKER: &str = "标准输出：\n";
const STDERR_MARKER: &str = "标准错误：\n";

/// 跳过 `命令：…` 行，使后续仍以 `退出码：` 行为准。
///
/// 只剥掉**第一个换行符之前**的片段；多行 argv 回显的续行仍留在结果开头，
/// 因此调用方不应假定返回值的首行就是状态行。
pub(super) fn strip_leading_command_invocation_line(output: &str) -> &str {
    let mut s = output.trim_start();
    if let Some(idx) = s.find('\n') {
        let first = s[..idx].trim();
        if first.starts_with("命令：") {
            s = s[idx + 1..].trim_start();
        }
    }
    s
}

pub(super) fn parse_exit_code(first_line: &str) -> Option<i32> {
    if let Some(rest) = first_line.strip_prefix("退出码：") {
        return rest.trim().parse::<i32>().ok();
    }
    let idx = first_line.find("(exit=")?;
    let rest = &first_line[idx + "(exit=".len()..];
    let end = rest.find(')')?;
    rest[..end].trim().parse::<i32>().ok()
}

/// 在首个 stdout/stderr 标记行之前逐行查找退出码。
///
/// 兜住「`命令：` 回显含换行」的情形：此时剥离后首行是参数正文而非状态行，
/// [`parse_exit_code`] 对首行必然失配，旧实现会退化成关键词启发式，把参数正文里的
/// 「失败」「超时」当成工具失败（`ok=false` 且 `exit_code=null`）。
///
/// 仅在输出确实具备 `run_command` 形态（出现流标记行）时采信，避免把其它工具的
/// 载荷正文（如 `read_file` 读到的文件内容）里的「退出码：N」误当状态。
pub(super) fn exit_code_in_header_region(output: &str) -> Option<i32> {
    let mut found = None;
    for line in output.lines() {
        let trimmed = line.trim();
        if STREAM_MARKERS.contains(&trimmed) {
            return found;
        }
        if found.is_none() {
            found = parse_exit_code(trimmed);
        }
    }
    // 遍历完仍无流标记行 ⇒ 不是 `run_command` 输出形态，不采信。
    None
}

pub(super) fn looks_like_failure(first_line: &str) -> bool {
    if first_line.is_empty() {
        return false;
    }
    first_line.starts_with("错误")
        || first_line.starts_with("未知工具")
        || first_line.starts_with("参数解析错误")
        || first_line.starts_with("执行失败")
        || first_line.contains("失败")
        || first_line.contains("超时")
}

pub(super) fn extract_streams(output: &str) -> (String, String) {
    let stdout = if let Some(pos) = output.find(STDOUT_MARKER) {
        let start = pos + STDOUT_MARKER.len();
        let end = output[start..]
            .find(STDERR_MARKER)
            .map(|i| start + i)
            .unwrap_or(output.len());
        output[start..end].trim().to_string()
    } else {
        String::new()
    };
    let stderr = if let Some(pos) = output.find(STDERR_MARKER) {
        let start = pos + STDERR_MARKER.len();
        output[start..].trim().to_string()
    } else {
        String::new()
    };
    (stdout, stderr)
}

pub(super) fn run_command_structured_payload(raw_output: &str) -> Value {
    let invocation = raw_output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("命令："))
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    let body = strip_leading_command_invocation_line(raw_output);
    let first = body.lines().next().unwrap_or("").trim();
    // 先按首行解析（单行命令的常态），失配再扫头部区域（多行回显的兜底）。
    let exit_code = parse_exit_code(first).or_else(|| exit_code_in_header_region(raw_output));
    let ok = if let Some(code) = exit_code {
        code == 0
    } else {
        !looks_like_failure(first)
    };
    let (stdout, stderr) = extract_streams(raw_output);
    serde_json::json!({
        "kind": "crabmate_structured_payload",
        "tool": "run_command",
        "version": 1_u64,
        "schema": "run_command_exit_v1",
        "invocation": invocation,
        "ok": ok,
        "exit_code": exit_code,
        "stdout_nonempty": !stdout.is_empty(),
        "stderr_nonempty": !stderr.is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::{exit_code_in_header_region, run_command_structured_payload};

    /// 回归守卫：`命令：` 回显含真实换行（多行 `-m`）时仍须取到退出码，
    /// 不得退化成「正文含『失败』即失败」。旧实现的 `structured_payload` 里
    /// `exit_code` 为 `null` 且 `ok=false`。
    #[test]
    fn multi_line_invocation_echo_still_reads_exit_code() {
        let raw = "命令：git commit -m \"refactor(x): 说明 / explain\" -m \"删除 28 份 .mdc，原会触发 check 失败；\n\n动机：三处副本必然漂移。\"\n退出码：0\n标准输出：\n[main abc1234] subject\n标准错误：\nall hooks Passed\n";
        let payload = run_command_structured_payload(raw);
        assert_eq!(payload["exit_code"], serde_json::json!(0));
        assert_eq!(payload["ok"], serde_json::json!(true));
    }

    /// 修复不得把非 0 退出码也判成成功。
    #[test]
    fn multi_line_invocation_echo_keeps_nonzero_failure() {
        let raw = "命令：git commit -m \"a\nb\"\n退出码：1\n标准输出：\n\n标准错误：\nhook failed\n";
        let payload = run_command_structured_payload(raw);
        assert_eq!(payload["exit_code"], serde_json::json!(1));
        assert_eq!(payload["ok"], serde_json::json!(false));
    }

    /// 非 `run_command` 形态的载荷正文里的「退出码：N」不得被采信。
    #[test]
    fn non_run_command_payload_ignores_exit_code_lines() {
        assert_eq!(exit_code_in_header_region("文件内容：\n退出码：130\n"), None);
        assert_eq!(exit_code_in_header_region("退出码：7\n没有流标记\n"), None);
    }

    /// 单行命令的常态路径不受影响。
    #[test]
    fn single_line_invocation_still_parsed() {
        let raw = "命令：pwd\n退出码：0\n标准输出：\n/\n";
        let payload = run_command_structured_payload(raw);
        assert_eq!(payload["exit_code"], serde_json::json!(0));
        assert_eq!(payload["ok"], serde_json::json!(true));
        assert_eq!(payload["stderr_nonempty"], serde_json::json!(false));
    }
}

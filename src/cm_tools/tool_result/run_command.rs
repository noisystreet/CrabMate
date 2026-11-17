//! `run_command` 输出解析：命令回显、退出码与 stdout/stderr 分流。
//!
//! 输出形状为 `命令：{argv}⏎退出码：{N}⏎标准输出：⏎…⏎标准错误：⏎…`。但 **`命令：`
//! 回显会把原始 argv 原样贴出**——多行参数（如 `git commit -m "多行正文"`）会让该行
//! 跨越数十行。因此「剥掉首行后首行即状态行」是不成立的假设，取退出码须用
//! [`exit_code_in_header_region`]（在首个流标记行之前逐行找）。

use serde_json::Value;
use crate::cm_names as tool_names;

use super::error_text;

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

/// **宽口径**判定：含「失败」「超时」即算失败。
///
/// 仍被这些路径使用——它们的输入是**工具正文**而非 `ToolError.message` 首行，没有
/// 「产生处补前缀」的保证：
/// - `body_after_header_looks_like_failure`（写预览正文逐行扫描）
/// - `run_command_structured_payload`
/// - `status.rs` 的三态判定
///
/// 因此它保留 `contains` 的双向失真（#1401 P1 记录在案）；④ 级首行判定请用
/// [`looks_like_failure_structured`]。
pub(super) fn looks_like_failure(first_line: &str) -> bool {
    if first_line.is_empty() {
        return false;
    }
    first_line.starts_with(error_text::ERROR_PREFIX)
        || first_line.starts_with(error_text::UNKNOWN_TOOL_PREFIX)
        || first_line.starts_with(error_text::INVALID_ARGS_PREFIX)
        || first_line.starts_with(error_text::EXECUTE_FAILED_PREFIX)
        || first_line.contains(error_text::FAILURE_MARKER)
        || first_line.contains(error_text::TIMEOUT_MARKER)
}

/// **收紧口径**（#1430）：只认结构化前缀，供 `parse_tool_output` 的 ④ 级兜底使用。
///
/// 前提是错误正文在产生处已统一补 `错误：`（`ToolError::build`）或本身带已知前缀
/// （`dispatch_tool` 的裸字符串也补过）。少了 `contains("失败")`，就不会把「正文提到
/// 失败」的成功输出误判为失败（`git log` 里的 `fix: 修复失败重试`）。
///
/// 已知残余漏判（属 #1430 第 3 步）：真实失败但文案是 `command not found` /
/// `permission denied` 的仍判成功，需要产生处带 `ok` / 退出码。
pub(super) fn looks_like_failure_structured(first_line: &str) -> bool {
    // 与 `ToolError::build` 的补前缀判据**同源**：那边保证「无前缀就补 `错误：`」，
    // 这边就认同一张表。这样新增前缀只需改 `KNOWN_ERROR_PREFIXES` 一处。
    error_text::has_known_error_prefix(first_line)
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
    let (invocation, body) = split_invocation_and_body(raw_output);
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
        "tool": tool_names::RUN_COMMAND,
        "version": 1_u64,
        "schema": "run_command_exit_v1",
        "invocation": invocation,
        "ok": ok,
        "exit_code": exit_code,
        "stdout_nonempty": !stdout.is_empty(),
        "stderr_nonempty": !stderr.is_empty(),
    })
}

/// 渲染正文里 invocation 的终止标记行（`CommandOutcome::render_body` 的固定格式）。
fn is_invocation_terminator(line: &str) -> bool {
    line == "标准输出："
        || line == "标准错误："
        || line == "(无输出)"
        || line.starts_with("退出码：")
}

/// 切出完整 invocation 与其后的正文。
///
/// 渲染格式是 `命令：{invocation}\n退出码：{code}\n[标准输出：…][标准错误：…]`，
/// 而 invocation **可以跨多行**（多行脚本、多行 commit message）。只取 `.lines().next()`
/// 会把它截断到第一行，下游据此展示或重放就只剩半截。超时与取消还会在最前面多一行提示，
/// 所以也不能直接看首行前缀。
///
/// 残余歧义：invocation 自身若有**一整行**恰好以 `退出码：` 开头（脚本正文里写死这种文本），
/// 会被当成边界。取第一个命中符合渲染顺序，且这种字面量极罕见。
fn split_invocation_and_body(raw_output: &str) -> (String, &str) {
    let Some(pos) = raw_output.find("命令：") else {
        return (String::new(), raw_output);
    };
    let tail = &raw_output[pos + "命令：".len()..];
    let mut invocation = String::new();
    let mut consumed = 0usize;
    for line in tail.split_inclusive('\n') {
        let stripped = line.strip_suffix('\n').unwrap_or(line);
        if is_invocation_terminator(stripped.trim()) {
            break;
        }
        if !invocation.is_empty() {
            invocation.push('\n');
        }
        invocation.push_str(stripped);
        consumed += line.len();
    }
    (invocation.trim().to_string(), &tail[consumed..])
}

#[cfg(test)]
mod tests {
    use super::{
        exit_code_in_header_region, looks_like_failure, looks_like_failure_structured,
        run_command_structured_payload,
    };

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

    /// 多行 invocation 必须完整保留：曾只取 `.lines().next()`，
    /// 多行脚本 / 多行 commit message 在载荷里只剩首行。
    #[test]
    fn multi_line_invocation_is_kept_whole() {
        let raw = "命令：printf '[%s]' \"line1\nline2\"\n退出码：0\n标准输出：\n[line1\nline2]\n";
        let payload = run_command_structured_payload(raw);
        assert_eq!(
            payload["invocation"],
            serde_json::json!("printf '[%s]' \"line1\nline2\"")
        );
        assert_eq!(payload["exit_code"], serde_json::json!(0));
    }

    /// 多行 commit message（`-m` 里带真实换行与 `$` 字面量）同样完整。
    #[test]
    fn multi_line_commit_message_invocation_is_kept_whole() {
        let raw =
            "命令：git commit -m 'add $foo $bar\nhehe'\n退出码：0\n标准输出：\n[main 1234] add\n";
        let payload = run_command_structured_payload(raw);
        assert_eq!(
            payload["invocation"],
            serde_json::json!("git commit -m 'add $foo $bar\nhehe'")
        );
    }

    /// 超时 / 取消时正文前多一行提示，invocation 仍要取到（曾因首行失配整段变空）。
    #[test]
    fn invocation_survives_timeout_prefix() {
        let raw = "命令执行超时（5 秒）；子进程已发送终止信号。\n命令：sleep 30\n退出码：-1\n";
        let payload = run_command_structured_payload(raw);
        assert_eq!(payload["invocation"], serde_json::json!("sleep 30"));
        assert_eq!(payload["exit_code"], serde_json::json!(-1));
    }

    /// **宽口径** `looks_like_failure` 的现状（#1401 P1 记录的双向失真，仍在）。
    ///
    /// 它继续服务「工具正文扫描」这类路径——输入没有「产生处补前缀」的保证，
    /// 所以 #1430 只收紧了 ④ 级，没有动这条。
    #[test]
    fn looks_like_failure_wide_still_has_both_gaps() {
        // ① 往失败偏：正文里带「失败」。
        assert!(looks_like_failure("fix: 修复失败重试"));
        // ② 往成功偏：真实失败但无中文失败词。
        assert!(!looks_like_failure("bash: foo: command not found"));
        assert!(!looks_like_failure("permission denied"));
    }

    /// ④ 级用的**收紧口径**（#1430）：只认结构化前缀，误判方向已消除。
    #[test]
    fn looks_like_failure_structured_only_trusts_prefixes() {
        // 成功正文里带「失败」不再误判。
        assert!(!looks_like_failure_structured("fix: 修复失败重试"));
        assert!(!looks_like_failure_structured("0 failed; 12 passed"));
        // 结构化前缀仍认——第 1 步保证错误正文都带其中之一。
        assert!(looks_like_failure_structured(
            "错误：读取文件失败: permission denied"
        ));
        assert!(looks_like_failure_structured("错误：工具执行超时（5 秒）"));
        assert!(looks_like_failure_structured("未知工具：nope"));
        assert!(looks_like_failure_structured(
            "参数解析错误：missing field `path`"
        ));
        assert!(looks_like_failure_structured("执行失败（cargo: not found）"));
        // 已知残余漏判（#1430 第 3 步）：无前缀的真实失败仍判成功。
        assert!(!looks_like_failure_structured(
            "bash: foo: command not found"
        ));
    }
}

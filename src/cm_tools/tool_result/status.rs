//! 工具成败的**唯一三态判定**：结构化信号优先，纯文案启发式只降级为 [`ToolStatus::Unknown`]。
//!
//! ## 为什么需要它
//!
//! 归前仓里有**三套**成败判定，缺省值互不相同，同一份输出在不同调用点能得出相反结论：
//!
//! | 实现 | 缺 `ok` 时 |
//! |---|---|
//! | `cm_memory::tool_check::tool_output_is_ok` | `false`（找不到 `退出码：0` 即失败） |
//! | `cm_memory::...::tool_output_semantic_success` | `true`（信封带 `ok` 就信，缺了直接放行） |
//! | `NormalizedToolEnvelope::from_crabmate_object` | 回退 `parse_tool_output`（最终落到正文启发式） |
//!
//! 根因是把**三态压成 `bool`**：「既无结构化信号、正文也读不出失败迹象」这种情况没有
//! 任何一个实现能表达，只能各自挑默认值。本模块给它 [`ToolStatus::Unknown`]。
//!
//! ## 判定顺序
//!
//! 1. `{"crabmate_tool":{…}}` 信封：显式 `ok` 优先，缺省则对 `output` 字段递归（只一层）；
//! 2. 有结构化证据（退出码，或首行 `crabmate_tool_output` 头）→ 直接采信 `parse_tool_output`；
//! 3. 只剩 `looks_like_failure` 这条纯文案启发式：说失败还算数，**说成功只能记 `Unknown`**。
//!
//! 第 3 步是本模块与旧实现的唯一语义差异，方向刻意偏保守：漏词时不下结论。
//! 真正删掉这条启发式需要先让「凡是工具输出都带结构化头」成立（见 issue #1401 P1 三步走）。
//!
//! ## 与旧 `tool_output_is_ok` 的口径差异（两处都是刻意变保守）
//!
//! - **退出码不再逐行扫**：旧实现全文找 `退出码：N`；本模块与 [`super::parse_tool_output`]
//!   同源，走 `exit_code_in_header_region`（须先出现 `标准输出：` / `标准错误：` 标记行才采信，
//!   否则会把 `read_file` 读到的文件内容里的「退出码：0」当成命令状态）。
//!   代价：没有流标记、`退出码：` 又不在首行的输出，从「成功」变成 `Unknown`。
//! - **判不出来不再当失败**：旧实现找不到成功信号即 `false`，本模块给 `Unknown`，
//!   由调用方各自决定（记忆沉淀不认恢复，缓存失效按「可能改过」）。

use super::run_command::{
    exit_code_in_header_region, looks_like_failure, parse_exit_code,
    strip_leading_command_invocation_line,
};
use super::{is_tool_output_header, parse_tool_output};

/// 工具调用结果的三态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Ok,
    Failed,
    /// 没有结构化信号、正文也不像失败——**判不出来**，由调用方按各自语义决定怎么办。
    ///
    /// 两个已知调用点方向相反，正好说明为什么不能替它选 `true` 或 `false`：
    /// 记忆沉淀不能把它当「已恢复」（否则凭空造经验），缓存失效则应把它当「可能改过」
    /// （宁可多算一次索引，也别留陈旧缓存）。
    Unknown,
}

/// 工具输出 → 三态。`content` 可以是整条 `role=tool` 消息（含 `crabmate_tool` 信封），
/// 也可以是工具返回的原始正文（首行可能带 `crabmate_tool_output` 头）。
pub fn tool_output_status(tool_name: &str, content: &str) -> ToolStatus {
    if let Some(status) = envelope_status(content) {
        return status;
    }
    let body = strip_leading_command_invocation_line(content);
    let first = body.lines().next().unwrap_or("").trim();
    let exit_code = parse_exit_code(first).or_else(|| exit_code_in_header_region(content));
    // 「有头无 `ok`」也算结构化证据：那是有意约定（错误路径不带头），旧实现同样按成功处理。
    let has_structured_evidence = exit_code.is_some() || is_tool_output_header(first);
    if has_structured_evidence {
        let parsed = parse_tool_output(tool_name, content);
        return if parsed.ok {
            ToolStatus::Ok
        } else {
            ToolStatus::Failed
        };
    }
    if looks_like_failure(first) {
        ToolStatus::Failed
    } else {
        ToolStatus::Unknown
    }
}

/// `{"crabmate_tool":{…}}` 信封 → 状态；非信封返回 `None`。
///
/// 刻意不复用 [`super::NormalizedToolEnvelope::ok`]：那是 `bool`，`Unknown` 已被拍成
/// `true`，正是本模块要消除的信息丢失。这里按同一优先序（显式 `ok` → 回退正文）重算，
/// 但保留三态。
///
/// 另：裸 JSON（非信封）自带的**顶层 `ok`** 也算结构化信号——旧 `tool_output_is_ok` 认它，
/// 丢了这个分支会让「JSON 里明写 `ok: false`」退化成 `Unknown`（`looks_like_failure`
/// 认不出英文 JSON）。
fn envelope_status(content: &str) -> Option<ToolStatus> {
    let v: serde_json::Value = serde_json::from_str(content.trim()).ok()?;
    if let Some(ct) = v.get("crabmate_tool").and_then(|x| x.as_object()) {
        if let Some(ok) = ct.get("ok").and_then(|x| x.as_bool()) {
            return Some(if ok { ToolStatus::Ok } else { ToolStatus::Failed });
        }
        // 缺 `output` 与 `NormalizedToolEnvelope` 保持一致：这不构成可解析的信封，
        // 交回正文路径；而不是拿空串递归、把结果固定成 `Unknown`。
        let output = ct.get("output").and_then(|x| x.as_str())?;
        let name = ct.get("name").and_then(|x| x.as_str()).unwrap_or("");
        // `output` 是工具正文，不会再套一层信封，所以递归深度恒为 1。
        return Some(tool_output_status(name, output));
    }
    let ok = v.get("ok").and_then(|x| x.as_bool())?;
    Some(if ok { ToolStatus::Ok } else { ToolStatus::Failed })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(ok: Option<bool>, output: &str) -> String {
        let mut ct = serde_json::json!({ "v": 1, "name": "run_command", "summary": "s", "output": output });
        if let Some(ok) = ok {
            ct["ok"] = serde_json::json!(ok);
        }
        serde_json::json!({ "crabmate_tool": ct }).to_string()
    }

    #[test]
    fn envelope_explicit_ok_wins() {
        assert_eq!(
            tool_output_status("run_command", &envelope(Some(true), "随便什么正文")),
            ToolStatus::Ok
        );
        assert_eq!(
            tool_output_status("run_command", &envelope(Some(false), "随便什么正文")),
            ToolStatus::Failed
        );
    }

    /// 信封缺 `ok` 时回退正文——这正是旧实现分歧的地方：`tool_output_is_ok` 会判 `false`。
    #[test]
    fn envelope_without_ok_falls_back_to_body() {
        assert_eq!(
            tool_output_status("run_command", &envelope(None, "命令：pwd\n退出码：0\n标准输出：\n/\n")),
            ToolStatus::Ok
        );
        assert_eq!(
            tool_output_status("run_command", &envelope(None, "命令：false\n退出码：1\n")),
            ToolStatus::Failed
        );
    }

    /// 结构化退出码优先，正文里出现「失败」字样不翻车。
    #[test]
    fn exit_code_beats_failure_wording() {
        let content = "命令：git log\n退出码：0\n标准输出：\nfix: 修复失败重试\n";
        assert_eq!(tool_output_status("run_command", content), ToolStatus::Ok);
    }

    /// 纯文案启发式只允许往「失败」偏；说成功一律降级 `Unknown`。
    #[test]
    fn text_only_evidence_yields_unknown_not_ok() {
        assert_eq!(
            tool_output_status("some_tool", "一段没有任何结构化信息的正文"),
            ToolStatus::Unknown
        );
        assert_eq!(
            tool_output_status("some_tool", "错误：未设置工作区"),
            ToolStatus::Failed
        );
    }

    /// 裸 JSON（非 `crabmate_tool` 信封）的顶层 `ok` 也算结构化信号。
    ///
    /// 丢了这条分支就会出现：JSON 里明写 `ok: false`，却因 `looks_like_failure` 认不出
    /// 英文 JSON 而被判成 `Unknown`。
    #[test]
    fn bare_json_uses_top_level_ok() {
        assert_eq!(
            tool_output_status("some_tool", r#"{"ok": false, "error": "boom"}"#),
            ToolStatus::Failed
        );
        assert_eq!(
            tool_output_status("some_tool", r#"{"ok": true, "value": 1}"#),
            ToolStatus::Ok
        );
        // 没有顶层 `ok` 的 JSON 不构成信号，仍是不下结论。
        assert_eq!(
            tool_output_status("some_tool", r#"{"value": 1}"#),
            ToolStatus::Unknown
        );
    }

    /// 信封缺 `output` 与 `NormalizedToolEnvelope` 口径一致：不当作可解析信封，
    /// 交回正文路径，而不是拿空串递归后固定成 `Unknown`。
    #[test]
    fn envelope_without_output_falls_back_to_legacy_path() {
        let raw = r#"{"crabmate_tool": {"v": 1, "name": "run_command"}}"#;
        assert_eq!(
            super::super::normalize_tool_message_content(raw),
            None,
            "NormalizedToolEnvelope 必须也认为它不是可解析信封"
        );
        assert_eq!(tool_output_status("run_command", raw), ToolStatus::Unknown);
        // 带 `output` 的同一形状才走信封回退。
        let with_output =
            r#"{"crabmate_tool": {"v": 1, "name": "run_command", "output": "命令：false\n退出码：1\n"}}"#;
        assert_eq!(
            tool_output_status("run_command", with_output),
            ToolStatus::Failed
        );
    }

    /// 无结构化证据时，三态与 `bool` 解析器的落点必须一致（`Unknown` 对应 `true`）。
    ///
    /// **别高估这条测试**：命中结构化证据的那几条是恒真的（`tool_output_status` 内部就是
    /// 直接调 `parse_tool_output` 再比 `ok`）。真正起守卫作用的是两条**无证据**输入——
    /// 它守的是「无证据分支不许自行猜成功」，不是「两个解析器全等」。
    /// 下面信封那组同理：比的是 `NormalizedToolEnvelope.ok` 的 bool 拍平口径。
    #[test]
    fn status_agrees_with_bool_parsers() {
        // 纯正文：与 `parse_tool_output` 同源。
        for content in [
            "命令：pwd\n退出码：0\n标准输出：\n/\n".to_string(),
            "命令：false\n退出码：1\n".to_string(),
            "一段没有任何结构化信息的正文".to_string(),
            "错误：未设置工作区".to_string(),
        ] {
            let status = tool_output_status("run_command", &content);
            let parsed = parse_tool_output("run_command", &content);
            assert_eq!(
                parsed.ok,
                status != ToolStatus::Failed,
                "三态与 `parse_tool_output` 分歧：{content}"
            );
        }
        // 信封：与 `NormalizedToolEnvelope` 同源（`parse_tool_output` 不认这种形状）。
        for content in [
            envelope(Some(true), "命令：pwd\n退出码：0\n"),
            envelope(Some(false), "命令：false\n退出码：1\n"),
            envelope(None, "命令：pwd\n退出码：0\n"),
            envelope(None, "命令：false\n退出码：1\n"),
        ] {
            let status = tool_output_status("run_command", &content);
            let env = super::super::normalize_tool_message_content(&content).expect("应为信封");
            assert_eq!(
                env.ok,
                status != ToolStatus::Failed,
                "三态与 `NormalizedToolEnvelope` 分歧：{content}"
            );
        }
    }
}

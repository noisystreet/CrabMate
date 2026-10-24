//! `read_file` / `modify_file` 的文件快照版本票据（CAS）：读取时发 revision，写前校验。
//!
//! revision 是文件**内容**的 SHA-256（`sha256:<64 位小写 hex>`），与行号快照绑定：
//! - `read_file` 在成功输出的 `crabmate_tool_output` 头里返回 `revision`；
//! - 同一轮内服务端还会按**规范化路径**记录最近一次成功读取的 revision；`modify_file`
//!   的行号类模式（`replace_lines` / `insert_after_line` / `edits`）默认拿它做写前校验，
//!   模型不传 `base_revision` 也生效，避免同一条消息内多个工具调用都基于旧快照、
//!   按已漂移的行号误改文件；
//! - 若本轮没有该文件的 `read_file` 记录且调用方未传 `base_revision`，行号类模式**拒绝写入**
//!   （`错误[missing_revision_baseline]`），不再静默放行；
//! - 显式 `base_revision` 优先且始终校验；`allow_stale_revision=true` 只跳过轮次自动绑定；
//! - `mode=full/overwrite` 不依赖行号，不做自动绑定。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::write_precheck::NOT_WRITTEN_HINT;
use crate::cm_tools::tools::ToolContext;

/// revision 前缀；比较时忽略前缀与大小写。
const REVISION_PREFIX: &str = "sha256:";
/// 流式计算 hash 的读缓冲区。
const REVISION_BUF_SIZE: usize = 256 * 1024;

/// revision 期望值的来源，决定错误提示措辞。
#[derive(Clone, Copy)]
enum RevisionSource {
    /// 调用方显式传入的 `base_revision`。
    Explicit,
    /// 本轮 `read_file` 自动记录的绑定。
    TurnRead,
}

/// 计算已打开文件的 revision；返回前把读写位置复位到文件开头（供后续按行读取）。
pub(super) fn revision_of_open_file(file: &mut File) -> std::io::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; REVISION_BUF_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(format!(
        "{REVISION_PREFIX}{}",
        hex_lower(&hasher.finalize())
    ))
}

/// 按路径计算 revision（写前校验用）。
pub(super) fn revision_of_path(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    revision_of_open_file(&mut file)
}

/// 读取可选的 `base_revision` 参数；空白视为未提供。
pub(super) fn base_revision_arg(v: &Value) -> Option<&str> {
    v.get("base_revision")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// `allow_stale_revision=true` 时跳过本轮自动绑定（显式 `base_revision` 仍会校验）。
pub(super) fn allow_stale_revision_arg(v: &Value) -> bool {
    v.get("allow_stale_revision")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
}

/// 写前 revision 校验：显式 `base_revision` 优先且始终校验；否则在 `auto_bind` 时
/// 使用本轮 `read_file` 记录的 revision；**没有记录即拒写**。`allow_stale_revision` 只影响后者。
pub(crate) fn verify_write_revision(
    v: &Value,
    target: &Path,
    ctx: &ToolContext<'_>,
    auto_bind: bool,
) -> Result<(), String> {
    if let Some(expected) = base_revision_arg(v) {
        return verify_revision(target, expected, RevisionSource::Explicit);
    }
    if !auto_bind || allow_stale_revision_arg(v) {
        return Ok(());
    }
    let Some(expected) = ctx
        .read_file_turn_cache
        .and_then(|c| c.last_read_revision(target))
    else {
        return Err(missing_revision_baseline(target));
    };
    verify_revision(target, &expected, RevisionSource::TurnRead)
}

/// 行号类编辑拿不到快照基线时的拒写提示。
///
/// 「本轮没读过该文件」曾直接放行，等于允许凭记忆写 `start_line` 落盘——恰是「按过期快照定位」
/// 最危险的形态。现在要求先取得基线，或由调用方显式承担风险（`allow_stale_revision=true`）。
fn missing_revision_baseline(target: &Path) -> String {
    format!(
        "错误[missing_revision_baseline]：行号类编辑缺少快照基线，拒绝写入（{NOT_WRITTEN_HINT}）。\n\
         路径：{}\n\
         原因：本轮尚未对目标文件调用 read_file，调用方也未传 base_revision，无法校验行号是否仍对应当前内容。\n\
         请先 read_file 该文件（输出头含 revision，服务端同时登记本轮绑定）再按当前行号修改；\
         或显式传入 read_file 得到的 base_revision；或改用 search_replace 按文本锚定编辑；\
         若确认要基于当前内容写入，可显式传 allow_stale_revision=true。",
        target.display()
    )
}

/// 校验 `expected` 与目标文件当前 revision；不一致返回可直接作为工具输出的错误正文。
fn verify_revision(
    target: &Path,
    expected: &str,
    source: RevisionSource,
) -> Result<(), String> {
    let current = revision_of_path(target).map_err(|e| {
        format!(
            "错误[revision_check_failed]：无法计算文件 revision: {e}（{NOT_WRITTEN_HINT}）。\n路径：{}",
            target.display()
        )
    })?;
    if normalize_revision(expected) == normalize_revision(&current) {
        return Ok(());
    }
    let path = target.display();
    let message = match source {
        RevisionSource::Explicit => format!(
            "错误[stale_revision]：目标文件自上次 read_file 后已变更，拒绝基于过期快照写入（{NOT_WRITTEN_HINT}）。\n\
             路径：{path}\n调用方 base_revision：{expected}\n当前 revision：{current}\n\
             请重新调用 read_file 获取最新内容与 revision，再按当前行号修改；或改用 search_replace 按文本锚定编辑。"
        ),
        RevisionSource::TurnRead => format!(
            "错误[stale_revision]：目标文件在本轮最近一次 read_file 之后已变更，行号快照已过期，拒绝写入（{NOT_WRITTEN_HINT}）。\n\
             路径：{path}\n本轮 read_file revision：{expected}\n当前 revision：{current}\n\
             请重新调用 read_file 获取最新内容与 revision，再按当前行号修改；或改用 search_replace 按文本锚定编辑；\
             若确认要基于当前内容跳过轮次自动校验，可显式传 allow_stale_revision=true。"
        ),
    };
    Err(message)
}

/// 比较用：去空白、忽略 `sha256:` 前缀与大小写。
fn normalize_revision(s: &str) -> String {
    let trimmed = s.trim().to_ascii_lowercase();
    match trimmed.strip_prefix(REVISION_PREFIX) {
        Some(digest) => digest.to_string(),
        None => trimmed,
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

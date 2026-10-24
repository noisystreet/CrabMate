//! 统一写盘提交入口：revision 校验紧贴 `rename` 前一刻，原子替换目标文件。
//!
//! 所有“覆盖已存在文件”的写路径都应经由此模块提交，而不是各自 `verify_*` 后再调
//! [`fs_atomic::commit_tmp_over_target`]：从校验到替换之间的窗口越短，其他进程/工具
//! 在同一文件上并发写入导致的 TOCTOU 窗口越小。

use std::fs::File;
use std::io::Write;
use std::path::Path;

use serde_json::Value;

use crate::cm_tools::fs_atomic;
use crate::cm_tools::tools::ToolContext;

use super::write_precheck::NOT_WRITTEN_HINT;

/// 提交临时文件：先做 revision 校验（显式 `base_revision` 始终校验；`auto_bind` 时
/// 使用本轮 `read_file` 绑定），再原子替换目标或写回原 inode。
///
/// 成功返回后临时文件已被消费（rename 或写回后删除），调用方应调用
/// [`fs_atomic::TmpFileGuard::keep`] 以避免 Drop 再删一次。
pub(super) fn commit_tmp_with_revision_guard(
    tmp_path: &Path,
    target: &Path,
    preserve_inode: bool,
    v: &Value,
    ctx: &ToolContext<'_>,
    auto_bind: bool,
) -> Result<(), String> {
    super::revision::verify_write_revision(v, target, ctx, auto_bind)?;
    fs_atomic::commit_tmp_over_target(tmp_path, target, preserve_inode)
}

/// 按字节覆盖目标文件：临时文件写入 → 继承元数据 → `fsync` → revision 校验 → 原子替换。
///
/// `target` 须已存在；多硬链接（`nlink > 1`）时退化为写回原 inode 以保留硬链接。
pub(crate) fn write_bytes_with_revision_guard(
    target: &Path,
    bytes: &[u8],
    v: &Value,
    ctx: &ToolContext<'_>,
    auto_bind: bool,
) -> Result<(), String> {
    let preserve_inode = fs_atomic::target_has_multiple_links(target);
    let mut guard = fs_atomic::TmpFileGuard::new(fs_atomic::unique_tmp_path(
        target,
        "crabmate_write_tmp",
    ));
    let mut file = File::create(guard.path())
        .map_err(|e| format!("创建临时文件失败: {e}（{NOT_WRITTEN_HINT}）"))?;
    if !preserve_inode {
        // 临时文件随后 rename 覆盖目标，需继承属主/权限/扩展属性，避免元数据退化。
        // 写回原 inode 时目标元数据不变，无需复制。
        fs_atomic::inherit_metadata_from_target(target, guard.path(), &file)?;
    }
    file.write_all(bytes)
        .map_err(|e| format!("写入临时文件失败: {e}（{NOT_WRITTEN_HINT}）"))?;
    file.sync_all()
        .map_err(|e| format!("落盘临时文件失败: {e}（{NOT_WRITTEN_HINT}）"))?;
    drop(file);
    commit_tmp_with_revision_guard(guard.path(), target, preserve_inode, v, ctx, auto_bind)?;
    guard.keep();
    Ok(())
}

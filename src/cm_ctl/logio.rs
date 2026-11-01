//! 实例日志的流式读取与轮转（#1213）。
//!
//! 两件事都只碰日志文件本身（不读记录、不碰进程），所以单独成模块，能用临时
//! 文件直接单测：
//!
//! - [`tail_lines`]：从文件尾按块往回读，凑够 N 行就停。旧实现用
//!   `read_to_string` 整份加载，日志涨到 GB 级后，排障的**第一条**命令
//!   （`crabmatectl logs`）就会吃满内存。
//! - [`rotate_if_oversized`]：起服务**前**按大小切一份，避免 `<name>.log`
//!   只增不减把宿主机磁盘占死。
//!
//! 刻意只保留**一份**旧日志（`.log.1`）：排障需要的是「重启前后各一段」，
//! 归档策略（压缩 / 多份 / 按时间戳命名）不在范围内（#1213 非目标）。

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 反向读的单块大小，同时也是单次分配上限。
const CHUNK_BYTES: usize = 8 * 1024;

/// 日志超过这个大小，就在下次起服务前轮转一次。
pub const ROTATE_THRESHOLD_BYTES: u64 = 32 * 1024 * 1024;

/// 轮转后旧日志的路径：`<name>.log` → `<name>.log.1`。
pub fn rotated_path(current: &Path) -> PathBuf {
    // 不用 `with_extension`：它会把 `.log` 整体替换掉，得到 `<name>.log.1` 是
    // 巧合而非语义（`a.b.log` 会变成 `a.b.log.1` 只在 extension 是单段时才成立）。
    let mut name = current.file_name().unwrap_or_default().to_os_string();
    name.push(".1");
    current.with_file_name(name)
}

/// 读日志尾部 `n` 行（`n` 按至少 1 处理）。
///
/// 反向按块读，凑够 `n + 1` 个换行就停：多要的那一个用来保证窗口首行
/// （可能被切在半行处）会被后面的 `take` 丢掉，所以输出与「整份读入再取尾部」
/// 一致。只有行数本来就不足 `n` 时才会一直读到文件头。
pub fn tail_lines(path: &Path, n: usize) -> io::Result<String> {
    let want = n.max(1);
    let mut file = File::open(path)?;
    let mut end = file.metadata()?.len();
    let mut buf: Vec<u8> = Vec::new();
    let mut newlines = 0usize;
    while end > 0 && newlines < want + 1 {
        let start = end.saturating_sub(CHUNK_BYTES as u64);
        let mut chunk = vec![0u8; (end - start) as usize];
        file.seek(SeekFrom::Start(start))?;
        let got = read_fully(&mut file, &mut chunk)?;
        chunk.truncate(got);
        newlines += chunk.iter().filter(|b| **b == b'\n').count();
        chunk.extend_from_slice(&buf);
        buf = chunk;
        end = start;
    }
    // 按字节窗口做 lossy 转换是安全的：半个多字节字符只可能出现在**首行**，
    // 而行数够时它会被 `start` 排除掉。
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(want);
    Ok(lines[from..].join("\n"))
}

/// 读「当前日志 + 上一份轮转日志」的尾部，凑够 `n` 行。
///
/// 轮转只保留一份，所以当前文件行数不足时要往前一份要：`logs -n 200` 不该
/// 因为刚轮转过就只剩两行。旧份不可读时退化为只给当前日志 —— 已经拿到手的
/// 内容不该因为「另一份有问题」而整条命令失败。
pub fn tail_with_rotated(current: &Path, n: usize) -> io::Result<String> {
    let want = n.max(1);
    let rotated = rotated_path(current);
    let text = match tail_lines(current, want) {
        Ok(t) => t,
        // 刚轮转、新日志还没建起来：上一份就是「当前」日志
        Err(e) if e.kind() == io::ErrorKind::NotFound => return tail_lines(&rotated, want),
        Err(e) => return Err(e),
    };
    let have = text.lines().count();
    if have >= want {
        return Ok(text);
    }
    let older = match tail_lines(&rotated, want - have) {
        Ok(t) => t,
        Err(_) => return Ok(text),
    };
    if older.is_empty() || text.is_empty() {
        return Ok(format!("{older}{text}"));
    }
    Ok(format!("{older}\n{text}"))
}

/// 起服务前按大小轮转：超过 `threshold` 就把当前日志改名为 `<name>.log.1`。
///
/// 返回是否真的切了（供单测与将来的诊断信息使用）。上一份直接覆盖 —— 只保留
/// 一份是刻意的，见模块注释。
pub fn rotate_if_oversized(path: &Path, threshold: u64) -> io::Result<bool> {
    let len = match fs::metadata(path) {
        Ok(m) => m.len(),
        // 首次启动还没有日志：无事可做
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if len < threshold {
        return Ok(false);
    }
    fs::rename(path, rotated_path(path))?;
    Ok(true)
}

/// 尽量填满 `chunk`；文件在读取期间被截断（并发轮转）时按实际读到长度返回。
fn read_fully(file: &mut File, chunk: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < chunk.len() {
        match file.read(&mut chunk[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cm_ctl_logio_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).expect("建临时目录");
        p
    }

    fn write_lines(path: &Path, rows: usize) {
        let mut text = String::new();
        for i in 1..=rows {
            text.push_str(&format!("line-{i}\n"));
        }
        std::fs::write(path, text).expect("写日志");
    }

    #[test]
    fn tail_returns_last_n_lines_without_trailing_newline() {
        let dir = tempdir("tail_basic");
        let log = dir.join("a.log");
        write_lines(&log, 5);
        assert_eq!(tail_lines(&log, 2).expect("应成功"), "line-4\nline-5");
    }

    /// 行数不足时返回全部（`lines()` 语义：末尾换行不产生空行）。
    #[test]
    fn tail_smaller_than_request_returns_everything() {
        let dir = tempdir("tail_all");
        let log = dir.join("a.log");
        write_lines(&log, 2);
        assert_eq!(tail_lines(&log, 100).expect("应成功"), "line-1\nline-2");
    }

    #[test]
    fn empty_log_yields_empty_string() {
        let dir = tempdir("tail_empty");
        let log = dir.join("a.log");
        std::fs::write(&log, "").expect("写空文件");
        assert_eq!(tail_lines(&log, 10).expect("应成功"), "");
    }

    #[test]
    fn missing_log_is_not_found() {
        let dir = tempdir("tail_missing");
        let err = tail_lines(&dir.join("nope.log"), 10).expect_err("应报不存在");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    /// 关键回归：日志远大于单块时**只读尾部**，且结果必须是最后 n 行。
    ///
    /// 这条同时钉住「跨块拼接顺序」：块是按从尾往头的顺序读的，拼错就会得到
    /// 倒序或重复行。
    #[test]
    fn tail_spans_multiple_chunks() {
        let dir = tempdir("tail_chunks");
        let log = dir.join("a.log");
        // 每行 ~64 字节，8000 行 ≈ 512 KiB，远超 8 KiB 的单块
        let mut text = String::new();
        for i in 1..=8000 {
            text.push_str(&format!("{i:0>58}\n"));
        }
        std::fs::write(&log, &text).expect("写日志");

        let got = tail_lines(&log, 3).expect("应成功");
        let all: Vec<&str> = text.lines().collect();
        let want = all[all.len() - 3..].join("\n");
        assert_eq!(got, want, "跨块读应给出连续的最后 3 行");
    }

    /// 多字节字符被切在块边界上时，不能把半个字符留在输出里。
    #[test]
    fn tail_keeps_utf8_boundaries_intact() {
        let dir = tempdir("tail_utf8");
        let log = dir.join("a.log");
        let mut text = String::new();
        for i in 1..=4000 {
            text.push_str(&format!("第 {i} 行：中文日志内容占多字节\n"));
        }
        std::fs::write(&log, &text).expect("写日志");

        let got = tail_lines(&log, 1).expect("应成功");
        assert_eq!(got, "第 4000 行：中文日志内容占多字节");
        assert!(!got.contains('\u{FFFD}'), "不应出现替换字符：{got}");
    }

    #[test]
    fn rotation_renames_oversized_log_and_keeps_one_copy() {
        let dir = tempdir("rotate");
        let log = dir.join("a.log");
        let rotated = rotated_path(&log);
        assert_eq!(rotated.file_name().expect("文件名").to_string_lossy(), "a.log.1");

        std::fs::write(&log, "0123456789").expect("写日志");
        assert!(
            !rotate_if_oversized(&log, 11).expect("轮转"),
            "未达阈值时不该切（阈值是「到这就切」的边界，10 < 11）"
        );

        assert!(rotate_if_oversized(&log, 8).expect("轮转"));
        assert!(!log.exists(), "当前日志应被改名");
        assert_eq!(std::fs::read_to_string(&rotated).expect("读旧份"), "0123456789");

        // 再切一次：旧份被覆盖，仍然只有一份
        std::fs::write(&log, "abcdefghij").expect("写日志");
        assert!(rotate_if_oversized(&log, 8).expect("轮转"));
        assert_eq!(std::fs::read_to_string(&rotated).expect("读旧份"), "abcdefghij");
    }

    #[test]
    fn rotation_of_missing_log_is_a_noop() {
        let dir = tempdir("rotate_missing");
        assert!(!rotate_if_oversized(&dir.join("nope.log"), 1).expect("应成功"));
    }

    /// 轮转边界：当前日志不足 n 行时，用上一份补齐，顺序为「旧 → 新」。
    #[test]
    fn tail_merges_rotated_file_to_fill_the_window() {
        let dir = tempdir("tail_rotated");
        let log = dir.join("a.log");
        std::fs::write(rotated_path(&log), "old-1\nold-2\nold-3\nold-4\n").expect("写旧份");
        std::fs::write(&log, "new-1\nnew-2\n").expect("写当前");

        assert_eq!(
            tail_with_rotated(&log, 3).expect("应成功"),
            "old-4\nnew-1\nnew-2"
        );
        // 当前文件已够 n 行：不碰旧份
        assert_eq!(tail_with_rotated(&log, 1).expect("应成功"), "new-2");
    }

    /// 刚轮转、新日志还没建：退化为读旧份。
    #[test]
    fn tail_falls_back_to_rotated_when_current_is_gone() {
        let dir = tempdir("tail_rotated_only");
        let log = dir.join("a.log");
        std::fs::write(rotated_path(&log), "only-old\n").expect("写旧份");
        assert_eq!(tail_with_rotated(&log, 10).expect("应成功"), "only-old");
    }
}

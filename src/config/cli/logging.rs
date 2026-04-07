//! `env_logger` 初始化与 `--log` 双写 stderr + 文件。

use std::io::{self, Write};
use std::path::Path;
use std::sync::Mutex;

/// 同时写 stderr 与日志文件（单条日志一份内容；关闭 ANSI 便于 `tail`）。
struct StderrAndFile {
    stderr: io::Stderr,
    file: std::fs::File,
}

impl Write for StderrAndFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stderr.write_all(buf)?;
        self.file.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stderr.flush()?;
        self.file.flush()
    }
}

/// 供 `env_logger::Target::Pipe` 使用的 `Write`（内部 `Mutex`）。
struct MutexWrite<W: Write + Send>(Mutex<W>);

impl<W: Write + Send> Write for MutexWrite<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut g = self
            .0
            .lock()
            .map_err(|_| io::Error::other("MutexWrite: 日志管道互斥锁已中毒（poisoned）"))?;
        g.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut g = self
            .0
            .lock()
            .map_err(|_| io::Error::other("MutexWrite: 日志管道互斥锁已中毒（poisoned）"))?;
        g.flush()
    }
}

fn open_log_append(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// 初始化 [`log`] + [`env_logger`]。
///
/// - 若已设置环境变量 **`RUST_LOG`**：完全按该变量解析（不强行覆盖默认级别）。
/// - 若未设置 **`RUST_LOG`**：
///   - 指定了 **`log_file`**（`--log <FILE>`）：默认 **`info`**，便于与文件 tail 配套；
///   - **`quiet_cli_default == true`**（非 `--serve` 的 CLI 模式：单次提问、REPL 等）：默认 **`warn`**，不输出 `info`；
///   - 否则（**`serve`**）：默认 **`info`**。
/// - 上述默认过滤器均带 **`tokei=error`**，避免依赖 **`tokei`** 在扫描未知扩展名时以 **`warn`** 刷屏（项目画像 / `code_stats` 等路径）。
///
/// 指定了 `--log` 但无法创建/打开日志文件时返回 [`io::Error`]，由调用方决定如何报告退出码。
pub fn init_logging(log_file: Option<&Path>, quiet_cli_default: bool) -> io::Result<()> {
    use env_logger::{Builder, Env, Target, WriteStyle};

    let env = if std::env::var_os("RUST_LOG").is_some() {
        Env::default()
    } else if log_file.is_some() {
        Env::default().default_filter_or("info,tokei=error")
    } else if quiet_cli_default {
        Env::default().default_filter_or("warn,tokei=error")
    } else {
        Env::default().default_filter_or("info,tokei=error")
    };
    let mut builder = Builder::from_env(env);
    builder.format_target(true);
    builder.format_timestamp_secs();
    match log_file {
        None => {
            builder.target(Target::Stderr);
        }
        Some(path) => {
            let f = open_log_append(path).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("无法打开日志文件 {}: {e}", path.display()),
                )
            })?;
            let w = MutexWrite(Mutex::new(StderrAndFile {
                stderr: io::stderr(),
                file: f,
            }));
            builder.target(Target::Pipe(Box::new(w)));
            builder.write_style(WriteStyle::Never);
        }
    }
    builder.init();
    Ok(())
}

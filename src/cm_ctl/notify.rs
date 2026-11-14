//! `sd_notify`：把「首轮收敛完成」告诉 systemd（`Type=notify` 的 `READY=1`）。
//!
//! 为什么手写而不是加 crate：systemd 的现代取值是**抽象命名空间**地址
//! （`@/org/freedesktop/systemd1/notify`，即 `sun_path[0] = 0`），std 的 `UnixDatagram`
//! 表达不了它；而 `libc` 已是 `server` feature 的既有依赖，手写几十行换来依赖面零变化
//! （`#1355` F 项曾以为要新增 crate）。
//!
//! 三条边界：
//!
//! - **未设 `NOTIFY_SOCKET` 就是 no-op**：手工在终端跑 `crabmatectl supervise --watch`
//!   的行为一字不变，因此不需要 `--no-notify` 之类的开关。
//! - **发送失败绝不中断 ctl**：notify 是「告诉别人我好了」，不是启动的前提条件。
//!   失败只在 stderr 提示一次，之后静默——否则退避重试会把日志刷满。
//! - **只发 `READY=1` / `STATUS=`**：`STOPPING=1` 与 `WatchdogSec` 需要信号处理，
//!   属另一个决策（见 `#1355` F 项的非目标）。

use std::cell::Cell;
use std::env;

/// 一个 `sockaddr_un` 里 `sun_path` 的可用字节数（Linux 为 108）。
///
/// 由类型尺寸推导而不是写死 108：结构体布局随平台变，写死会与 `sun_path` 越界检查错位。
const SUN_PATH_LEN: usize =
    std::mem::size_of::<libc::sockaddr_un>() - std::mem::size_of::<libc::sa_family_t>();

/// `NOTIFY_SOCKET` 的发送端。未配置时所有方法都是 no-op。
#[derive(Debug, Clone, Default)]
pub struct Notifier {
    socket: Option<String>,
    /// 是否已经因发送失败警告过（只警告一次，见模块注释）。
    warned: Cell<bool>,
}

impl Notifier {
    /// 按环境变量构造（`systemd` 起服务时自动设置）。
    pub fn from_env() -> Self {
        Self::from_spec(env::var("NOTIFY_SOCKET").ok())
    }

    /// 显式指定地址。
    ///
    /// 单测专用入口：**不读也不改进程环境变量**，否则 `cargo test` 的并行线程会互相串味
    /// （`set_var` 影响同进程所有测试）。
    pub fn from_spec(spec: Option<String>) -> Self {
        Self {
            socket: spec.filter(|s| !s.trim().is_empty()),
            warned: Cell::new(false),
        }
    }

    /// 是否真的有可发送的目标（`NOTIFY_SOCKET` 已设置）。
    pub fn is_enabled(&self) -> bool {
        self.socket.is_some()
    }

    /// 就绪：`READY=1` + 一行 `STATUS=`。
    ///
    /// `status` 会显示在 `systemctl --user status` 里，是「首轮收敛到底成了几个」的唯一可见面。
    pub fn ready(&self, status: &str) {
        self.send(&format!("READY=1\nSTATUS={status}"));
    }

    /// 只更新进度（`STATUS=`），不触碰 `READY`。
    pub fn status(&self, text: &str) {
        self.send(&format!("STATUS={text}"));
    }

    fn send(&self, payload: &str) {
        let Some(spec) = self.socket.as_deref() else {
            return;
        };
        if let Err(e) = send_datagram(spec, payload)
            && !self.warned.replace(true)
        {
            eprintln!("WARN: sd_notify 发送失败（不影响 ctl 继续运行）：{e}");
        }
    }
}

/// 向 `NOTIFY_SOCKET` 发一条数据报。
fn send_datagram(spec: &str, payload: &str) -> Result<(), String> {
    let (addr, len) = encode_addr(spec)?;
    // SAFETY: 只操作本函数持有的 fd 与栈上的 `sockaddr_un`。
    // - `socket()` 的失败立即返回，成功路径上 fd 在 `close` 前不逃逸；
    // - `sendto` 的地址指针指向 `addr`，长度 `len` 由 `encode_addr` 按 `sockaddr_un`
    //   的真实字段布局算出（不超过结构体尺寸）；
    // - 载荷指针与长度取自 `payload.as_bytes()`，调用期间 `payload` 保持存活且未被移动。
    unsafe {
        let fd = libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return Err(format!(
                "socket(AF_UNIX) 失败：{}",
                std::io::Error::last_os_error()
            ));
        }
        let sent = libc::sendto(
            fd,
            payload.as_ptr().cast(),
            payload.len(),
            0,
            (&raw const addr).cast(),
            len,
        );
        // 先取 errno 再 close：close 本身可能覆盖 errno
        let err = if sent < 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        libc::close(fd);
        match err {
            Some(e) => Err(format!("sendto({spec}) 失败：{e}")),
            None => Ok(()),
        }
    }
}

/// 把 `NOTIFY_SOCKET` 的取值编码成 `sockaddr_un` + `socklen_t`。
///
/// 纯函数、无系统调用：单测直接对字节断言，不必真的起 systemd。
///
/// 两个容易写错的地方：
///
/// - `@` 前缀是**抽象**命名空间（`sun_path[0]` 必须为 0，名字紧随其后），它不是文件路径；
/// - 长度算法不同：路径名要含结尾 NUL，抽象地址要含那个前导 0、但不需要结尾 NUL。
///   算错任一处，内核会按另一种形式解析，表现为 `ENOENT` / `ECONNREFUSED` 而非明显错误。
fn encode_addr(spec: &str) -> Result<(libc::sockaddr_un, libc::socklen_t), String> {
    let raw = spec.as_bytes();
    let (offset, name) = match raw.split_first() {
        Some((b'@', rest)) => (1usize, rest),
        _ => (0usize, raw),
    };
    if name.is_empty() {
        return Err(format!("NOTIFY_SOCKET 为空：{spec}"));
    }
    // 路径名要额外留一个 NUL，抽象地址不用
    let reserved = if offset == 0 { 1 } else { 0 };
    let cap = SUN_PATH_LEN - offset - reserved;
    if name.len() > cap {
        // 截断会静默变成「另一个 socket」，宁可报错
        return Err(format!(
            "NOTIFY_SOCKET 过长（{} 字节，上限 {cap}）：{spec}",
            name.len()
        ));
    }

    // SAFETY: `sockaddr_un` 全零是合法取值（`sun_path` 全零即空字符串），
    // `sun_family` 随后被覆盖，不依赖任何未初始化数据。
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in addr.sun_path[offset..].iter_mut().zip(name) {
        *slot = *byte as libc::c_char;
    }

    let len = std::mem::size_of::<libc::sa_family_t>() + offset + name.len() + reserved;
    Ok((addr, len as libc::socklen_t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::tempdir;

    #[test]
    fn encodes_pathname_with_trailing_nul() {
        let spec = "/run/systemd/notify";
        let (addr, len) = encode_addr(spec).expect("应可编码");
        assert_eq!(addr.sun_family, libc::AF_UNIX as libc::sa_family_t);
        assert_eq!(addr.sun_path[0], b'/' as libc::c_char);
        // 结尾那个 NUL 必须算进长度，否则内核按「非终止字符串」处理
        let expect = std::mem::size_of::<libc::sa_family_t>() + spec.len() + 1;
        assert_eq!(len as usize, expect, "路径名长度要含结尾 NUL");
        assert_eq!(addr.sun_path[spec.len()], 0, "sun_path 里应留出 NUL");
    }

    #[test]
    fn encodes_abstract_socket_with_leading_nul() {
        let spec = "@/org/freedesktop/systemd1/notify";
        let (addr, len) = encode_addr(spec).expect("应可编码");
        assert_eq!(addr.sun_path[0], 0, "抽象地址第一个字节必须是 0");
        let name = &spec[1..];
        assert_eq!(addr.sun_path[1], b'/' as libc::c_char);
        for (i, b) in name.as_bytes().iter().enumerate() {
            assert_eq!(addr.sun_path[1 + i], *b as libc::c_char, "第 {i} 字节不符");
        }
        // 抽象地址：前缀(family) + 那个 0 + 名字，不含额外 NUL
        let expect = std::mem::size_of::<libc::sa_family_t>() + 1 + name.len();
        assert_eq!(len as usize, expect);
    }

    #[test]
    fn rejects_empty_and_overlong_specs() {
        assert!(encode_addr("").is_err(), "空串不是合法地址");
        assert!(encode_addr("@").is_err(), "只有 `@` 等于空名字");
        // 路径名可用 108-1 字节；再长就必须报错而不是截断
        let overlong = format!("/{}", "a".repeat(SUN_PATH_LEN));
        let err = encode_addr(&overlong).expect_err("超长应报错");
        assert!(err.contains("过长"), "错误信息要说明原因：{err}");
        // 恰好卡在上限的应通过
        let at_limit = format!("/{}", "a".repeat(SUN_PATH_LEN - 2));
        assert!(encode_addr(&at_limit).is_ok(), "上限内不该被拒");
    }

    #[test]
    fn without_socket_is_silent_noop() {
        let n = Notifier::from_spec(None);
        assert!(!n.is_enabled());
        // 未配置时既不能 panic，也不该留下"警告过"的状态
        n.ready("无 NOTIFY_SOCKET");
        n.status("进度");
        assert!(!n.warned.get());
        // 空串与纯空白等同未设置（systemd 不会给这种值，但环境变量被手改过时会）
        assert!(!Notifier::from_spec(Some("   ".to_string())).is_enabled());
    }

    #[test]
    fn send_failure_is_swallowed_and_warned_once() {
        // 指向不存在的 socket：必须**不中断** ctl，只警告一次
        let n = Notifier::from_spec(Some("/nonexistent/crabmate-notify.sock".to_string()));
        assert!(n.is_enabled());
        n.ready("首轮收敛完成");
        assert!(n.warned.get(), "首次失败应置位 warned");
        n.status("第二次");
        // 第二次仍失败，但 warned 已置位 → 不再刷日志（无法直接断言 stderr，只断言状态）
        assert!(n.warned.get());
    }

    #[test]
    fn delivers_ready_datagram_to_real_socket() {
        // 端到端：真实 Unix datagram 收到 `READY=1`（编码 + sendto 的组合验证）。
        // 抽象地址在测试里 bind 不便，其编码已由上面的字节级断言覆盖。
        let dir = tempdir("notify_deliver");
        let sock_path = dir.join("notify.sock");
        let receiver = std::os::unix::net::UnixDatagram::bind(&sock_path).expect("绑定接收端");
        let n = Notifier::from_spec(Some(sock_path.to_string_lossy().into_owned()));
        n.ready("1/1 实例就绪");

        let mut buf = [0u8; 256];
        let got = receiver.recv(&mut buf).expect("应收到数据报");
        let text = String::from_utf8_lossy(&buf[..got]).into_owned();
        assert!(text.starts_with("READY=1\n"), "首行必须是 READY=1：{text}");
        assert!(
            text.contains("STATUS=1/1 实例就绪"),
            "STATUS 要带上收敛结果：{text}"
        );
        assert!(!n.warned.get(), "成功发送不该置位 warned");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

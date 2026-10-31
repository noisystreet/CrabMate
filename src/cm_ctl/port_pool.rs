//! 端口池：按顺序取第一个空闲端口，**全占用时明确报错**（不随机挑，便于排查）。
//!
//! 探测与真正 `bind` 之间存在 TOCTOU 窗口，因此 `up` 在 readiness 超时后
//! 还会用 [`PortPool::next_free_from`] 顺延重试一次（见 `ops`）。

use std::net::TcpListener;

use super::CtlError;

/// 默认端口池起止（含端点）。
pub const DEFAULT_LO: u16 = 8080;
pub const DEFAULT_HI: u16 = 8089;

/// 端口池。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortPool {
    lo: u16,
    hi: u16,
}

impl PortPool {
    /// 由给定区间构造（校验 `lo != 0` 且 `lo <= hi`）。仅测试使用——生产只走 [`Self::default_pool`]。
    #[cfg(test)]
    pub fn new(lo: u16, hi: u16) -> Result<Self, CtlError> {
        if lo == 0 {
            return Err(CtlError::usage("bad_port_range", "端口下限不能为 0"));
        }
        if lo > hi {
            return Err(CtlError::usage(
                "bad_port_range",
                format!("端口池区间非法：{lo}-{hi}（下限须 ≤ 上限）"),
            ));
        }
        Ok(Self { lo, hi })
    }

    pub fn default_pool() -> Self {
        Self {
            lo: DEFAULT_LO,
            hi: DEFAULT_HI,
        }
    }

    #[cfg(test)]
    pub fn lo(&self) -> u16 {
        self.lo
    }

    #[cfg(test)]
    pub fn hi(&self) -> u16 {
        self.hi
    }

    pub fn describe(&self) -> String {
        format!("{}-{}", self.lo, self.hi)
    }

    /// 端口当前是否可绑定。
    ///
    /// 绑定成功即视为空闲；探测后立即释放监听（窗口很小但不为零，故 `up` 另有顺延重试）。
    pub fn is_free(port: u16) -> bool {
        TcpListener::bind(("127.0.0.1", port)).is_ok()
    }

    /// 取池内第一个空闲端口；全部占用时返回 `port_pool_exhausted`。
    pub fn first_free(&self) -> Result<u16, CtlError> {
        for port in self.lo..=self.hi {
            if Self::is_free(port) {
                return Ok(port);
            }
        }
        Err(CtlError::general(
            "port_pool_exhausted",
            format!("端口池 {} 已无空闲端口", self.describe()),
        ))
    }

    /// 从 `after` 之后顺延找下一个空闲端口（环绕、跳过 `after` 自身）。
    ///
    /// 用于「刚被抢走端口」的竞争重试。
    pub fn next_free_from(&self, after: u16) -> Result<u16, CtlError> {
        let span = u32::from(self.hi - self.lo) + 1;
        for step in 1..=span {
            let offset = (u32::from(after.saturating_sub(self.lo)) + step) % span;
            let port = self.lo + offset as u16;
            if port != after && Self::is_free(port) {
                return Ok(port);
            }
        }
        Err(CtlError::general(
            "port_pool_exhausted",
            format!("端口池 {} 已无其它空闲端口", self.describe()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_inverted_range() {
        let err = PortPool::new(8090, 8080).expect_err("应拒绝倒置区间");
        assert_eq!(err.error_code, "bad_port_range");
    }

    #[test]
    fn rejects_zero_lower_bound() {
        assert!(PortPool::new(0, 10).is_err());
    }

    #[test]
    fn accepts_single_port_range() {
        let p = PortPool::new(8080, 8080).expect("单端口区间应合法");
        assert_eq!(p.describe(), "8080-8080");
    }

    #[test]
    fn busy_port_is_not_free() {
        // 占住一个由系统分配的端口，它必然不可再绑定
        let held = TcpListener::bind(("127.0.0.1", 0)).expect("应可绑定随机端口");
        let port = held.local_addr().expect("应可取地址").port();
        assert!(!PortPool::is_free(port), "被占用的端口不应判为空闲");
    }

    #[test]
    fn first_free_returns_port_in_range() {
        let pool = PortPool::new(49152, 49200).expect("合法区间");
        let port = pool.first_free().expect("随机高位端口段应有空闲");
        assert!((pool.lo()..=pool.hi()).contains(&port), "应在池内：{port}");
    }

    #[test]
    fn next_free_skips_reference_port() {
        let pool = PortPool::new(49152, 49200).expect("合法区间");
        let first = pool.first_free().expect("应有空闲");
        let next = pool.next_free_from(first).expect("应能顺延");
        assert_ne!(next, first, "顺延结果不应等于出发点");
        assert!((pool.lo()..=pool.hi()).contains(&next));
    }

    #[test]
    fn exhausted_pool_reports_clear_error() {
        // 占住一个端口后，用只含它的池去取 → 必然耗尽
        let held = TcpListener::bind(("127.0.0.1", 0)).expect("应可绑定随机端口");
        let port = held.local_addr().expect("应可取地址").port();
        let pool = PortPool::new(port, port).expect("单端口池");
        let err = pool.first_free().expect_err("该端口被占，应报耗尽");
        assert_eq!(err.error_code, "port_pool_exhausted");
        assert!(err.message.contains(&port.to_string()));
    }
}

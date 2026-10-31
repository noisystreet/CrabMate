//! `crabmatectl`：本地多实例 `serve` 进程管理器（Linux-only）。
//!
//! 设计要点（见 `docs/命令行与路由.md` 与 `docs/开发文档.md`）：
//!
//! - **纯本地、无网络面**：只操作本机进程与本地文件，不监听 TCP；
//! - **每实例独立 cwd 与会话库**：`serve` 的 `conversation_store_sqlite_path` 与
//!   `.serve.lock` 按**进程 cwd** 相对解析，同 cwd 起第二个实例会直接失败，
//!   故子进程 cwd 一律指向该实例专属目录；
//! - **状态文件是缓存，`--json` 才是契约**：记录只增不删、容忍未知与缺失字段；
//! - **发信号前先验身份**：`pid` / `pgid` 是历史值，PID 复用会误杀无关进程组，
//!   故 `boot_id` + `pid_start_ticks` + `cmdline` 三重校验（见 `identity` 模块）。

pub(crate) mod control;
pub(crate) mod guard;
pub(crate) mod identity;
pub(crate) mod instances;
pub(crate) mod lock;
pub(crate) mod ops;
pub(crate) mod paths;
pub(crate) mod port_pool;
pub(crate) mod preflight;
pub(crate) mod probe;
pub(crate) mod profiles;
pub(crate) mod readiness;
pub(crate) mod record;
pub(crate) mod relaunch;
pub(crate) mod render;
pub(crate) mod restart;
pub(crate) mod run;
pub(crate) mod signal;
pub(crate) mod spawn;
pub(crate) mod supervise;

pub use run::run;

/// 本平台的进程管理能力是否完整（目前仅 Linux）。
///
/// 进程身份与存活判定（`boot_id` / `pid_start_ticks` / `cmdline` / `pid_alive`）
/// 全部读 `/proc`，而 macOS 没有 procfs。在那里 `pid_alive` 会恒为 `false`，于是
/// `down` 把「读不到事实」当成「进程已死」：清掉 `pid` 并写 `stopped`，进程却仍在
/// 后台跑；`ls` 同样把每个实例都报成 `stopped`，`--restart on-failure` 更会每轮
/// 巡检再起一个新进程。
///
/// 与其让这种静默误判成立，不如在**入口**（`run`）直接拒绝：拿不到进程事实时说
/// 「不支持」，比给出一个编造的状态诚实。
pub const PLATFORM_SUPPORTED: bool = cfg!(target_os = "linux");

use std::fmt;

/// `ctl` 的统一错误：退出码 + 机读 `error_code` + 人读 message。
///
/// 细分的失败原因走 [`Self::error_code`] 而不是继续消耗全局退出码空间
/// （`EXIT_MODEL_ERROR` / `EXIT_QUOTA_OR_RATE_LIMIT` 等已被其它子命令占用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtlError {
    pub exit_code: i32,
    pub error_code: &'static str,
    pub message: String,
}

impl CtlError {
    /// 一般失败（I/O、启动失败、身份校验不符等）。
    pub fn general(error_code: &'static str, message: impl Into<String>) -> Self {
        Self {
            exit_code: crate::EXIT_GENERAL,
            error_code,
            message: message.into(),
        }
    }

    /// 用法/参数错误。
    pub fn usage(error_code: &'static str, message: impl Into<String>) -> Self {
        Self {
            exit_code: crate::EXIT_USAGE,
            error_code,
            message: message.into(),
        }
    }

    /// 「这个地址/名字已经被占着」类冲突：`port_in_use` / `instance_already_live`。
    ///
    /// 与 [`Self::usage`] 共用退出码（`EXIT_USAGE`），因为对调用方而言两者是同一件事：
    /// **现在重试没有意义**，得先处理掉占用的那个东西（换端口 / 先 `down`）。把它们分到
    /// 两个退出码上，脚本就得同时判断 1 和 2 才能认出「别重试」。
    ///
    /// `identity_mismatch` 刻意不走这里：它不是「被占用」，而是「记录不可信、需人工核对」，
    /// 属 [`Self::general`] 那一类（重试同样无意义，但补救动作完全不同）。
    pub fn conflict(error_code: &'static str, message: impl Into<String>) -> Self {
        Self {
            exit_code: crate::EXIT_USAGE,
            error_code,
            message: message.into(),
        }
    }
}

impl fmt::Display for CtlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}（{}）", self.message, self.error_code)
    }
}

impl std::error::Error for CtlError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_error_uses_usage_exit_code() {
        let e = CtlError::usage("bad_arg", "参数非法");
        assert_eq!(e.exit_code, crate::EXIT_USAGE);
        assert!(e.to_string().contains("bad_arg"));
    }

    #[test]
    fn general_error_uses_general_exit_code() {
        let e = CtlError::general("lock_busy", "锁被占用");
        assert_eq!(e.exit_code, crate::EXIT_GENERAL);
    }

    /// 读 `fixtures/ctl_contract.json` 的一组字段（已排序）。
    fn expected_keys(field: &str) -> Vec<String> {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ctl_contract.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读 {} 失败：{e}", path.display()));
        let v: serde_json::Value = serde_json::from_str(&raw).expect("golden 应是合法 JSON");
        let mut keys: Vec<String> = v[field]
            .as_array()
            .unwrap_or_else(|| panic!("golden 缺少数组字段 `{field}`"))
            .iter()
            .map(|k| {
                k.as_str()
                    .unwrap_or_else(|| panic!("`{field}` 里应全是字符串"))
                    .to_string()
            })
            .collect();
        keys.sort();
        keys
    }

    fn sorted_keys(v: &serde_json::Value) -> Vec<String> {
        let mut keys: Vec<String> = v
            .as_object()
            .expect("契约对象应是 JSON object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    fn sample_probe() -> probe::Probe {
        probe::observe(&record::InstanceRecord::default())
    }

    /// `ls --json` 的每行字段集由 `crabmate-client` 直接消费。
    ///
    /// 改名 / 删字段 → 客户端静默读到 `null`（表现为 lane 一直「未知」），且没有任何
    /// 编译期或运行期信号；新增字段 → 内部结构被泄漏成契约、以后删不掉。两个方向都
    /// 必须显式改 `fixtures/ctl_contract.json`，那一步就是审阅点。
    #[test]
    fn ls_row_json_matches_the_contract() {
        let rec = record::InstanceRecord::default();
        let v = render::row_json(&rec, &sample_probe());
        assert_eq!(sorted_keys(&v), expected_keys("ls_row"));
    }

    /// `status --json` = `ls` 的一行 + 进程身份三项。
    #[test]
    fn status_json_matches_the_contract() {
        let rec = record::InstanceRecord::default();
        let v = render::status_json(&rec, &sample_probe());
        assert_eq!(sorted_keys(&v), expected_keys("status_row"));
    }

    /// 状态文件（`<name>.json`）同样是契约：客户端直接读它，不只是读 `ls` 输出。
    /// 这条也顺带盯住「别把只该活在内存里的字段写进盘」。
    #[test]
    fn record_fields_match_the_contract() {
        let v = serde_json::to_value(record::InstanceRecord::default()).expect("序列化记录");
        assert_eq!(sorted_keys(&v), expected_keys("record_fields"));
    }
}

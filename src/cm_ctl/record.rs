//! 实例状态记录：跨版本**只增不删**的本地缓存。
//!
//! 兼容承诺是「容忍未知字段与缺失字段」，而不是「字段永久冻结」：脚本与客户端
//! 应消费 `ctl ls --json`，状态文件只用于断线后仍可读（ctl 未运行时也能看到上次状态）。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::CtlError;

/// 记录格式版本（单调整数；**不**用 crate 版本 —— 后者会因无关改动 bump）。
pub const SCHEMA_VERSION: u32 = 1;

/// 落盘的实例状态（`stale` 不在此列：它是读取时派生的事实，见 `probe`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InstanceState {
    #[default]
    Starting,
    Ready,
    Failed,
    Stopped,
}

impl InstanceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
}

/// 期望态：`supervise` 依据它决定「该不该把实例拉起来」。
///
/// 没有这个字段时，用户 `down` 掉的实例会被常驻的 `supervise` 立刻重启。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DesiredState {
    #[default]
    Running,
    Stopped,
}

impl DesiredState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
        }
    }
}

/// 单个实例的状态记录。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstanceRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub state: InstanceState,
    #[serde(default)]
    pub desired_state: DesiredState,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub pid: Option<u32>,
    /// 进程组 id：`down` 对整组发信号，避免留下 `clang-format` / `cmake` 等工具子进程。
    #[serde(default)]
    pub pgid: Option<i32>,
    /// 写入时的开机标识，用于识别「记录来自上次开机」。
    #[serde(default)]
    pub boot_id: Option<String>,
    /// `/proc/<pid>/stat` 第 22 字段：与 `boot_id` 一起唯一确定一个进程身份。
    #[serde(default)]
    pub pid_start_ticks: Option<u64>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub config_in_use: Option<String>,
    /// 实际传给子进程的环境变量**名**（不含值），用于核对没有隐式继承。
    #[serde(default)]
    pub env_keys: Vec<String>,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub config: Option<String>,
    #[serde(default)]
    pub log: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub profile_in_use: Option<String>,
    /// 谁负责这个实例：`cli`（人工 `up` / `run`）或 `supervise`（声明式收敛）。
    ///
    /// 必须存在而不是靠「声明里有没有」反推：`supervise` 回收「声明已删掉」的实例时，
    /// 只能回收自己拉起的那批。否则一旦有人手工 `crabmatectl up` 过某个实例，
    /// 收敛就会把它当成「多余声明」杀掉 —— 那是不可接受的破坏。
    #[serde(default)]
    pub managed_by: Option<String>,
    /// 仅供排查，不参与兼容判断。
    #[serde(default)]
    pub ctl_version: Option<String>,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

impl InstanceRecord {
    pub fn new(name: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            name: name.to_string(),
            state: InstanceState::Starting,
            desired_state: DesiredState::Running,
            ctl_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            ..Default::default()
        }
    }

    /// 记录是否声明「应当运行」（`supervise` 收敛判据）。
    pub fn wants_running(&self) -> bool {
        self.desired_state == DesiredState::Running
    }
}

/// 原子写入：同目录临时文件 + `rename`（同目录保证不跨文件系统），权限 `0600`。
pub fn write_atomic(path: &Path, rec: &InstanceRecord) -> Result<(), CtlError> {
    let dir = path.parent().ok_or_else(|| {
        CtlError::general(
            "state_dir_unwritable",
            format!("状态文件路径无父目录：{}", path.display()),
        )
    })?;
    let body = serde_json::to_vec_pretty(rec).map_err(|e| {
        CtlError::general("record_serialize", format!("序列化状态记录失败：{e}"))
    })?;

    let tmp = dir.join(format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("record"),
        std::process::id()
    ));

    fs::write(&tmp, &body).map_err(|e| {
        CtlError::general(
            "state_dir_unwritable",
            format!("写入临时状态文件失败 {}：{e}", tmp.display()),
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600)).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            CtlError::general(
                "state_dir_unwritable",
                format!("设置状态文件权限 0600 失败 {}：{e}", tmp.display()),
            )
        })?;
    }

    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        CtlError::general(
            "state_dir_unwritable",
            format!("替换状态文件失败 {}：{e}", path.display()),
        )
    })?;
    Ok(())
}

/// 读取状态记录；文件不存在返回 `Ok(None)`。
pub fn read_optional(path: &Path) -> Result<Option<InstanceRecord>, CtlError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(CtlError::general(
                "record_unreadable",
                format!("读取状态文件失败 {}：{e}", path.display()),
            ));
        }
    };
    let rec: InstanceRecord = serde_json::from_slice(&bytes).map_err(|e| {
        CtlError::general(
            "record_unreadable",
            format!("解析状态文件失败 {}：{e}", path.display()),
        )
    })?;
    Ok(Some(rec))
}

/// 读取状态记录；不存在视为错误（`up`/`down`/`status` 需要它存在）。
pub fn read_required(path: &Path) -> Result<InstanceRecord, CtlError> {
    read_optional(path)?.ok_or_else(|| {
        CtlError::general(
            "instance_not_found",
            format!("实例记录不存在：{}", path.display()),
        )
    })
}

pub fn remove(path: &Path) -> Result<(), CtlError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CtlError::general(
            "record_unwritable",
            format!("删除状态文件失败 {}：{e}", path.display()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cm_ctl_record_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&p).expect("建临时目录");
        p
    }

    #[test]
    fn roundtrip_via_atomic_write() {
        let dir = tmpdir();
        let path = dir.join("a.json");
        let mut rec = InstanceRecord::new("a");
        rec.state = InstanceState::Ready;
        rec.port = Some(8080);
        rec.base_url = Some("http://127.0.0.1:8080".to_string());
        write_atomic(&path, &rec).expect("写入应成功");

        let back = read_required(&path).expect("读取应成功");
        assert_eq!(back.name, "a");
        assert_eq!(back.state, InstanceState::Ready);
        assert_eq!(back.schema_version, SCHEMA_VERSION);
        assert!(back.wants_running());

        // 不留临时文件
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .expect("列目录")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tolerates_unknown_and_missing_fields() {
        let dir = tmpdir();
        let path = dir.join("b.json");
        // 缺几乎所有字段 + 多一个未知字段
        fs::write(
            &path,
            br#"{"name":"b","state":"ready","future_field":123}"#,
        )
        .expect("写最小记录");
        let rec = read_required(&path).expect("应容忍未知与缺失字段");
        assert_eq!(rec.name, "b");
        assert_eq!(rec.state, InstanceState::Ready);
        assert_eq!(rec.schema_version, SCHEMA_VERSION, "缺失时用默认版本");
        assert_eq!(rec.desired_state, DesiredState::Running);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_none_not_error() {
        let dir = tmpdir();
        let got = read_optional(&dir.join("nope.json")).expect("不应报错");
        assert!(got.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_required_reports_not_found() {
        let dir = tmpdir();
        let err = read_required(&dir.join("nope.json")).expect_err("应报不存在");
        assert_eq!(err.error_code, "instance_not_found");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_is_idempotent() {
        let dir = tmpdir();
        let path = dir.join("c.json");
        remove(&path).expect("不存在时删除也应成功");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_serializes_as_snake_case() {
        let rec = InstanceRecord {
            state: InstanceState::Stopped,
            desired_state: DesiredState::Stopped,
            ..InstanceRecord::new("x")
        };
        let s = serde_json::to_string(&rec).expect("序列化");
        assert!(s.contains("\"stopped\""));
        assert!(!rec.wants_running());
    }
}

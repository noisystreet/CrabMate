//! XDG 路径解析、目录权限、实例名校验。
//!
//! 状态目录与配置目录按 **`0700`** 创建：状态文件含 `last_error`（可能带路径）
//! 与 `workspace`，属本机使用者私有数据。

use std::fs;
use std::path::{Path, PathBuf};

use path_absolutize::Absolutize;

use super::CtlError;

/// 实例名最大长度（用于文件名，过长不利于排查）。
const MAX_NAME_LEN: usize = 64;

/// 状态/配置/运行时目录集合。
#[derive(Debug, Clone)]
pub struct CtlPaths {
    state_dir: PathBuf,
    config_dir: PathBuf,
    runtime_dir: Option<PathBuf>,
}

impl CtlPaths {
    /// 按 XDG 规范解析；缺失时回退 `$HOME` 下的约定路径。
    pub fn resolve() -> Result<Self, CtlError> {
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            CtlError::general("no_home", "无法确定 HOME：请设置环境变量 HOME")
        })?;

        let state_dir = xdg_dir("XDG_STATE_HOME")
            .map(|d| d.join("crabmate"))
            .unwrap_or_else(|| home.join(".local").join("state").join("crabmate"));

        let config_dir = xdg_dir("XDG_CONFIG_HOME")
            .map(|d| d.join("crabmate"))
            .unwrap_or_else(|| home.join(".config").join("crabmate"));

        let runtime_dir = xdg_dir("XDG_RUNTIME_DIR").map(|d| d.join("crabmate"));

        Ok(Self {
            state_dir,
            config_dir,
            runtime_dir,
        })
    }

    /// 测试用：把状态根指向给定目录（字段私有，故由本模块提供构造器）。
    #[cfg(test)]
    pub fn for_test(root: &Path) -> Self {
        Self {
            state_dir: root.to_path_buf(),
            config_dir: root.join("config"),
            runtime_dir: None,
        }
    }

    /// 声明文件路径：`$XDG_CONFIG_HOME/crabmate/instances.toml`。
    ///
    /// 放在**配置**目录而不是状态目录：它是人手写的输入，不是 ctl 产出的缓存。
    pub fn instances_file(&self) -> PathBuf {
        self.config_dir.join(super::instances::FILE_NAME)
    }

    /// 控制面 socket 路径：`$XDG_RUNTIME_DIR/crabmate/ctl.sock`。
    ///
    /// `XDG_RUNTIME_DIR` 缺失时回退到状态目录，而不是 `/tmp`：`/tmp` 全局可写，
    /// 把「起进程的入口」放进去等于凭空多出一道只能靠 sticky 位兜的边界。
    pub fn control_socket(&self) -> PathBuf {
        self.runtime_dir
            .clone()
            .unwrap_or_else(|| self.state_dir.clone())
            .join("ctl.sock")
    }

    /// 准备控制面目录（`0700`）并返回 socket 路径。
    pub fn prepare_control_socket(&self) -> Result<PathBuf, CtlError> {
        let sock = self.control_socket();
        if let Some(dir) = sock.parent() {
            create_dir_0700(dir)?;
        }
        Ok(sock)
    }

    /// 预设文件路径：`$XDG_CONFIG_HOME/crabmate/profiles.toml`。
    pub fn profiles_file(&self) -> PathBuf {
        self.config_dir.join(super::profiles::FILE_NAME)
    }

    /// 状态文件所在目录：`<state>/crabmate/instances`。
    pub fn instances_dir(&self) -> PathBuf {
        self.state_dir.join("instances")
    }

    /// 实例专属 home（子进程 **cwd**，见模块注释：会话库与 serve 锁按 cwd 解析）。
    pub fn instance_home(&self, name: &str) -> PathBuf {
        self.instances_dir().join(name)
    }

    /// 子实例日志：`<instances>/<name>.log`。
    pub fn log_path(&self, name: &str) -> PathBuf {
        self.instances_dir().join(format!("{name}.log"))
    }

    /// 状态文件路径。
    pub fn record_path(&self, name: &str) -> Result<PathBuf, CtlError> {
        validate_name(name)?;
        Ok(self.instances_dir().join(format!("{name}.json")))
    }

    /// 创建状态目录、实例目录与配置目录（`0700`）。
    pub fn ensure_dirs(&self) -> Result<(), CtlError> {
        create_dir_0700(&self.state_dir)?;
        create_dir_0700(&self.instances_dir())?;
        if let Some(rt) = &self.runtime_dir {
            create_dir_0700(rt)?;
        }
        Ok(())
    }

    /// 整个实例目录组（prune 时用）。
    pub fn all_record_paths(&self) -> Result<Vec<PathBuf>, CtlError> {
        let dir = self.instances_dir();
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(CtlError::general(
                    "state_dir_unreadable",
                    format!("读取状态目录失败 {}：{e}", dir.display()),
                ));
            }
        };
        let mut out = Vec::new();
        for ent in rd {
            let ent = match ent {
                Ok(v) => v,
                Err(_) => continue,
            };
            let p = ent.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                out.push(p);
            }
        }
        out.sort();
        Ok(out)
    }
}

fn xdg_dir(var: &str) -> Option<PathBuf> {
    let v = std::env::var_os(var)?;
    let p = PathBuf::from(v);
    if p.as_os_str().is_empty() {
        None
    } else {
        Some(p)
    }
}

pub(super) fn create_dir_0700(path: &Path) -> Result<(), CtlError> {
    if path.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(path).map_err(|e| {
        CtlError::general(
            "state_dir_unwritable",
            format!("创建目录失败 {}：{e}", path.display()),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o700);
        fs::set_permissions(path, perms).map_err(|e| {
            CtlError::general(
                "state_dir_unwritable",
                format!("设置目录权限 0700 失败 {}：{e}", path.display()),
            )
        })?;
    }
    Ok(())
}

/// 私有文件闸门：仅本人可读写（`0600` 或更严）且属主是当前用户。
///
/// 用于那些**等同权限清单**的本地文件（`instances.toml` / `profiles.toml`）：
/// 同机其他用户若能写它们，就能借 ctl 起一个指向任意目录、带任意 `allowed_commands`
/// 的 serve，绕开文件系统本来就挡住的访问。存在性由调用方先判断（缺失语义各不相同）。
pub fn check_private_file(
    path: &Path,
    what: &str,
    insecure_code: &'static str,
    foreign_owner_code: &'static str,
) -> Result<(), CtlError> {
    use std::os::unix::fs::MetadataExt;

    let meta = fs::metadata(path).map_err(|e| {
        CtlError::general(
            "file_metadata_failed",
            format!("读取{what}元数据失败 {}：{e}", path.display()),
        )
    })?;

    let mode = meta.mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(CtlError::general(
            insecure_code,
            format!(
                "拒绝加载{what} {}：权限 {mode:03o} 过宽（要求 0600 或更严）。\n\
                 它等同进程启动清单——同机其他用户若能写它，就能借 ctl 起指向任意目录、带任意权限的 serve；\n\
                 请执行 `chmod 600 {}` 后重试",
                path.display(),
                path.display()
            ),
        ));
    }

    // 取 euid 不想引 libc + unsafe：Linux 上 `/proc/self` 目录的 uid 就是当前 euid。
    // 拿不到（macOS 无 procfs、容器裁剪）时跳过本项，权限位检查仍然生效。
    let Ok(self_meta) = fs::metadata("/proc/self") else {
        return Ok(());
    };
    let euid = self_meta.uid();
    if meta.uid() == euid {
        return Ok(());
    }
    Err(CtlError::general(
        foreign_owner_code,
        format!(
            "拒绝加载{what} {}：属主 uid {} 与当前用户 {euid} 不符",
            path.display(),
            meta.uid()
        ),
    ))
}

/// 实例名 → 文件名安全：只允许字母数字与 `.` `_` `-`，不以 `.` 开头，长度受限。
///
/// 拒绝路径分隔符与 `..`，避免借实例名做目录穿越。
pub fn validate_name(name: &str) -> Result<(), CtlError> {
    if name.is_empty() {
        return Err(CtlError::usage("bad_name", "实例名不能为空"));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(CtlError::usage(
            "bad_name",
            format!("实例名过长（上限 {MAX_NAME_LEN} 字符）：{name}"),
        ));
    }
    if name.starts_with('.') {
        return Err(CtlError::usage(
            "bad_name",
            format!("实例名不能以 `.` 开头：{name}"),
        ));
    }
    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if !ok {
        return Err(CtlError::usage(
            "bad_name",
            format!("实例名只允许 ASCII 字母数字与 `.` `_` `-`：{name}"),
        ));
    }
    Ok(())
}

/// 绝对化路径：`serve` 内部**不**规范化 `--workspace`，ctl 必须在记录与传参前做。
pub fn absolutize(p: &str) -> Result<PathBuf, CtlError> {
    let trimmed = p.trim();
    if trimmed.is_empty() {
        return Err(CtlError::usage("bad_path", "路径不能为空"));
    }
    Path::new(trimmed)
        .absolutize()
        .map(|c| c.into_owned())
        .map_err(|e| CtlError::usage("bad_path", format!("无法解析路径 {trimmed}：{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_accepts_reasonable_names() {
        for n in ["proj-a", "proj_a", "a.b", "x1"] {
            assert!(validate_name(n).is_ok(), "{n} 应被接受");
        }
    }

    #[test]
    fn validate_name_rejects_path_traversal() {
        for n in ["", "..", "../etc", "a/b", ".hidden", "a b"] {
            assert!(validate_name(n).is_err(), "{n} 应被拒绝");
        }
    }

    #[test]
    fn validate_name_rejects_overlong() {
        let n = "a".repeat(MAX_NAME_LEN + 1);
        let err = validate_name(&n).expect_err("应拒绝过长实例名");
        assert_eq!(err.error_code, "bad_name");
    }

    #[test]
    fn absolutize_makes_relative_absolute() {
        let p = absolutize("some/rel").expect("应可绝对化");
        assert!(p.is_absolute(), "{} 应为绝对路径", p.display());
    }

    #[test]
    fn absolutize_rejects_empty() {
        assert!(absolutize("   ").is_err());
    }

    #[test]
    fn instance_dirs_are_stable() {
        let paths = CtlPaths {
            state_dir: PathBuf::from("/s"),
            config_dir: PathBuf::from("/c"),
            runtime_dir: None,
        };
        assert_eq!(
            paths.record_path("a").expect("名字合法"),
            PathBuf::from("/s/instances/a.json")
        );
        assert_eq!(paths.instance_home("a"), PathBuf::from("/s/instances/a"));
        assert_eq!(paths.log_path("a"), PathBuf::from("/s/instances/a.log"));
    }
}

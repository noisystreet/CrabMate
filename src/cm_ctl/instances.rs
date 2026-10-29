//! `instances.toml`：`supervise --from-config` 的声明文件解析与权限校验。
//!
//! 这个文件**等同一份进程启动清单**（决定起什么进程、带哪个 `workspace`），
//! 因此不能当普通配置对待：
//!
//! 1. **权限必须 `0600`（或更严）**：同机其他用户若能写它，就能借 ctl 起一个指向
//!    任意目录的 serve，从而绕开文件系统本来就挡住的访问。这条检查就是 `ctl` 与
//!    「让 `serve` 暴露 `/instances` HTTP 端点」在攻击面上的主要差别。
//! 2. **未知键一律报错**，不静默忽略：把 `enabled` 拼错会让实例「莫名其妙不起来」，
//!    而错误的配置被静默接受时，排查成本远高于启动时直接失败。
//! 3. **`workspace` 是唯一由外部提供的运行时值**，越界判定交给
//!    [`super::preflight`]（那里比对的是**子实例**的 `workspace_allowed_roots`），
//!    本模块只管格式与权限。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::CtlError;
use super::paths;

/// 声明文件名：`$XDG_CONFIG_HOME/crabmate/instances.toml`。
pub const FILE_NAME: &str = "instances.toml";

/// 声明文件顶层。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstancesFile {
    #[serde(default)]
    pub instances: BTreeMap<String, Declaration>,
}

/// 单个实例声明。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    /// 工作区路径（支持 `~/...`）。是否在允许根内由子实例配置决定。
    pub workspace: String,
    /// 交给子 serve 的 `--config`；相对路径按 **ctl 的 cwd** 绝对化。
    #[serde(default)]
    pub config: Option<String>,
    /// 指定端口；省略则由端口池分配。
    #[serde(default)]
    pub port: Option<u16>,
    /// `false` 表示「声明上保留、但不该运行」——客户端摘 lane 时的软删除语义。
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 显式环境（敏感名由 [`super::spawn::build_env`] 拒绝，绝不静默透传）。
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

fn default_enabled() -> bool {
    true
}

impl Declaration {
    /// 声明里的 workspace → 绝对路径。
    ///
    /// 先展开 `~`：`Path::absolutize` 只补 cwd，不认 shell 的波浪号，而声明文件是
    /// 人手写的（`workspace = "~/proj-a"`），不展开会解析成 `<cwd>/~/proj-a`。
    pub fn workspace_path(&self) -> Result<PathBuf, CtlError> {
        paths::absolutize(&expand_tilde(self.workspace.trim()))
    }
}

fn expand_tilde(raw: &str) -> String {
    if raw != "~" && !raw.starts_with("~/") {
        return raw.to_string();
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return raw.to_string();
    };
    if raw == "~" {
        return home.display().to_string();
    }
    // `~/` 是纯 ASCII 前缀，`[2..]` 一定落在字符边界上
    home.join(&raw[2..]).display().to_string()
}

/// 读取并校验声明文件。
pub fn load(path: &Path) -> Result<InstancesFile, CtlError> {
    check_permissions(path)?;
    let text = std::fs::read_to_string(path).map_err(|e| {
        CtlError::general(
            "instances_file_unreadable",
            format!("读取声明文件失败 {}：{e}", path.display()),
        )
    })?;
    let parsed: InstancesFile = toml::from_str(&text).map_err(|e| {
        CtlError::usage("instances_file_invalid", invalid_message(path, &e.to_string()))
    })?;
    validate(&parsed)?;
    Ok(parsed)
}

fn invalid_message(path: &Path, detail: &str) -> String {
    let hint = if detail.contains("profile") {
        "\n提示：`profile` 属于 supervise 的后续切片；当前版本请直接写 `workspace` / `config` / `port` / `enabled` / `env`"
    } else {
        ""
    };
    format!("解析声明文件失败 {}：{detail}{hint}", path.display())
}

/// 声明项自身的格式校验（名字必须能安全用作文件名 + workspace 非空）。
fn validate(file: &InstancesFile) -> Result<(), CtlError> {
    for (name, decl) in &file.instances {
        if let Err(e) = paths::validate_name(name) {
            return Err(CtlError::usage(
                "bad_instances_entry",
                format!("声明项 `{name}` 非法：{}", e.message),
            ));
        }
        if decl.workspace.trim().is_empty() {
            return Err(CtlError::usage(
                "bad_instances_entry",
                format!("声明项 `{name}` 缺少 workspace"),
            ));
        }
    }
    Ok(())
}

/// 权限闸门：文件必须仅本人可读写（`0600` 或更严），且属于当前用户。
fn check_permissions(path: &Path) -> Result<(), CtlError> {
    use std::os::unix::fs::MetadataExt;

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CtlError::usage(
                "instances_file_missing",
                format!(
                    "声明文件不存在：{}（`supervise --from-config` 需要它）",
                    path.display()
                ),
            ));
        }
        Err(e) => {
            return Err(CtlError::general(
                "instances_file_unreadable",
                format!("读取声明文件元数据失败 {}：{e}", path.display()),
            ));
        }
    };

    let mode = meta.mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(CtlError::general(
            "instances_file_insecure",
            format!(
                "拒绝加载声明文件 {}：权限 {mode:03o} 过宽（要求 0600 或更严）。\n\
                 它等同进程启动清单——同机其他用户若能写它，就能借 ctl 起指向任意目录的 serve；\n\
                 请执行 `chmod 600 {}` 后重试",
                path.display(),
                path.display()
            ),
        ));
    }
    check_owner(path, meta.uid())
}

fn check_owner(path: &Path, owner: u32) -> Result<(), CtlError> {
    // 取 euid 不想引 libc + unsafe：Linux 上 `/proc/self` 目录的 uid 就是当前 euid。
    // 拿不到（macOS 无 procfs、容器裁剪）时跳过本项检查，权限位检查仍然生效。
    use std::os::unix::fs::MetadataExt;
    let Ok(self_meta) = std::fs::metadata("/proc/self") else {
        return Ok(());
    };
    let euid = self_meta.uid();
    if owner == euid {
        return Ok(());
    }
    Err(CtlError::general(
        "instances_file_foreign_owner",
        format!(
            "拒绝加载声明文件 {}：属主 uid {owner} 与当前用户 {euid} 不符",
            path.display()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cm_ctl_instances_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).expect("建临时目录");
        p
    }

    fn write_with_mode(path: &Path, body: &str, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, body).expect("写声明文件");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("设权限");
    }

    const OK_BODY: &str = r#"
[instances.proj-a]
workspace = "/tmp/proj-a"
port = 8080
"#;

    #[test]
    fn loads_minimal_declaration_with_enabled_default_true() {
        let dir = tmpdir("minimal");
        let p = dir.join(FILE_NAME);
        write_with_mode(&p, OK_BODY, 0o600);
        let f = load(&p).expect("应加载成功");
        let d = f.instances.get("proj-a").expect("应有 proj-a");
        assert!(d.enabled, "enabled 缺省为 true");
        assert_eq!(d.port, Some(8080));
        assert_eq!(d.workspace, "/tmp/proj-a");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_missing_file() {
        let dir = tmpdir("missing");
        let err = load(&dir.join(FILE_NAME)).expect_err("缺失应报错");
        assert_eq!(err.error_code, "instances_file_missing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_world_writable_file() {
        let dir = tmpdir("insecure");
        let p = dir.join(FILE_NAME);
        write_with_mode(&p, OK_BODY, 0o644);
        let err = load(&p).expect_err("0644 必须拒绝");
        assert_eq!(err.error_code, "instances_file_insecure");
        assert!(err.message.contains("chmod 600"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_unknown_key_instead_of_ignoring() {
        let dir = tmpdir("unknown");
        let p = dir.join(FILE_NAME);
        write_with_mode(
            &p,
            "[instances.a]\nworkspace = \"/tmp/a\"\nenabledd = true\n",
            0o600,
        );
        let err = load(&p).expect_err("未知键必须报错");
        assert_eq!(err.error_code, "instances_file_invalid");
        assert!(err.message.contains("enabledd"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn profile_key_reports_it_as_a_later_slice() {
        let dir = tmpdir("profile");
        let p = dir.join(FILE_NAME);
        write_with_mode(
            &p,
            "[instances.a]\nworkspace = \"/tmp/a\"\nprofile = \"readonly-audit\"\n",
            0o600,
        );
        let err = load(&p).expect_err("profile 尚未实现，应报错");
        assert!(err.message.contains("后续切片"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_empty_workspace() {
        let dir = tmpdir("empty_ws");
        let p = dir.join(FILE_NAME);
        write_with_mode(&p, "[instances.a]\nworkspace = \"  \"\n", 0o600);
        let err = load(&p).expect_err("空 workspace 应报错");
        assert_eq!(err.error_code, "bad_instances_entry");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_unsafe_instance_name() {
        let dir = tmpdir("bad_name");
        let p = dir.join(FILE_NAME);
        write_with_mode(&p, "[instances.\"../evil\"]\nworkspace = \"/tmp/a\"\n", 0o600);
        let err = load(&p).expect_err("路径穿越名应报错");
        assert_eq!(err.error_code, "bad_instances_entry");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_path_expands_tilde() {
        let d = Declaration {
            workspace: "~/proj-a".to_string(),
            config: None,
            port: None,
            enabled: true,
            env: BTreeMap::new(),
        };
        let p = d.workspace_path().expect("应可绝对化");
        assert!(p.is_absolute(), "{}", p.display());
        if std::env::var_os("HOME").is_some() {
            assert!(!p.to_string_lossy().contains('~'), "{}", p.display());
        }
    }
}

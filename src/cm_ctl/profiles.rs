//! `profiles.toml`：supervise 的**预设菜单**。
//!
//! 为什么不让实例直接写 `config = "<任意路径>"` 就完事：两个硬伤。
//!
//! 1. **跨端不可行**：Web / 移动端不知道服务端文件系统，拼不出合法路径；
//! 2. **没有上界**：允许「按任意路径加载配置」等于允许声明方把任意 TOML
//!    塞进子进程，能力集合无界。
//!
//! profile 把这件事收敛成菜单：配置由**声明方预先定义**，实例只引用名字。
//!
//! 字段白名单是硬约束，两条永久禁止：
//!
//! - **`workspace`** —— 只由实例声明提供，并经 `preflight` 按子实例的
//!   `workspace_allowed_roots` 校验；profile 携带它就成了绕过白名单的通道。
//! - **`yes` / `--yes`** —— 命令白名单全放行必须是**实例级显式决定**，
//!   藏进预设里等于让「读一个 TOML」变成提权。
//!
//! 未知键一律报错（`deny_unknown_fields`），不做静默忽略。

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::CtlError;
use super::paths;

/// 文件名：`$XDG_CONFIG_HOME/crabmate/profiles.toml`。
pub const FILE_NAME: &str = "profiles.toml";

/// `CM_PERMISSION_LEVEL` 的合法取值（与 `cm_config` 一致）。
const PERMISSION_LEVELS: &[&str] = &["chat", "read", "workspace", "approved", "unrestricted"];

/// profiles 文件顶层。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilesFile {
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

/// 一个预设方案（固定菜单里的一项）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// 交给子 serve 的 `--config`。实例不能再单独给 `config`（两者互斥）。
    #[serde(default)]
    pub config: Option<String>,
    /// 命令白名单（整体替换，不是追加）→ `CM_ALLOWED_COMMANDS`。
    #[serde(default)]
    pub allowed_commands: Option<Vec<String>>,
    /// 权限等级 → `CM_PERMISSION_LEVEL`。
    #[serde(default)]
    pub permission_level: Option<String>,
    /// 禁用全部工具 → 子进程的 `--no-tools`。
    #[serde(default)]
    pub no_tools: Option<bool>,
    /// 模型 id → `CM_LLM_MODEL`。
    #[serde(default)]
    pub llm_model: Option<String>,
    /// 网关基址 → `CM_LLM_API_BASE`。
    #[serde(default)]
    pub llm_api_base: Option<String>,
    /// 显式环境；敏感名（`API_KEY` / `*_TOKEN` / `*_SECRET` / 白名单外 `CM_*`）
    /// 在 spawn 时被 `forbidden_env` 拒绝。
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// profile 解析后的可执行参数（已与实例声明合并）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    pub serve_config: Option<String>,
    pub env: BTreeMap<String, String>,
    pub no_tools: bool,
}

impl Resolved {
    /// 生效集合的稳定指纹（FNV-1a 64）。
    ///
    /// 用来回答「运行中的实例用的是不是**当前**这份 profile」。只比名字不够：
    /// 改 `allowed_commands` / `permission_level` / `env` 时名字一字未变，
    /// 而权限集合已经不同了。用自写的 FNV 而不是 `DefaultHasher`，是因为后者
    /// 不保证跨 Rust 版本稳定，那会让升级二进制后**每个**实例都被判定为变更。
    pub fn fingerprint(&self) -> String {
        let mut parts = vec![
            format!("config={}", self.serve_config.as_deref().unwrap_or("")),
            format!("no_tools={}", self.no_tools),
        ];
        // `BTreeMap` 已按 key 有序，序列化是确定的
        for (k, v) in &self.env {
            parts.push(format!("{k}={v}"));
        }
        fnv1a_64(&parts.join("\u{1f}"))
    }
}

fn fnv1a_64(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// 读取 profiles 文件；**不存在**视为「没有预设」（`Ok(empty)`）。
///
/// 只有被实例引用时缺失才算错（报 `profile_not_found` 并给出查找路径），
/// 这样不用 profile 的用户完全不必创建这个文件。
pub fn load_optional(path: &Path) -> Result<ProfilesFile, CtlError> {
    match std::fs::metadata(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ProfilesFile::default()),
        Err(e) => {
            return Err(CtlError::general(
                "profiles_file_unreadable",
                format!("读取 profiles 文件元数据失败 {}：{e}", path.display()),
            ));
        }
    }
    // 与 instances.toml 同一闸门：能写它就能给子进程任意 allowed_commands / 权限等级
    paths::check_private_file(
        path,
        "profiles 文件",
        "profiles_file_insecure",
        "profiles_file_foreign_owner",
    )?;

    let text = std::fs::read_to_string(path).map_err(|e| {
        CtlError::general(
            "profiles_file_unreadable",
            format!("读取 profiles 文件失败 {}：{e}", path.display()),
        )
    })?;
    let parsed: ProfilesFile = toml::from_str(&text).map_err(|e| {
        CtlError::usage(
            "profiles_file_invalid",
            invalid_message(path, &e.to_string()),
        )
    })?;
    validate(&parsed)?;
    Ok(parsed)
}

fn invalid_message(path: &Path, detail: &str) -> String {
    let quoted = |k: &str| detail.contains(&format!("`{k}`")) || detail.contains(&format!("'{k}'"));
    let hint = if quoted("workspace") {
        "（`workspace` 只能由实例声明提供，profile 不得携带）"
    } else if quoted("yes") || quoted("--yes") {
        "（白名单全放行必须由实例级显式决定，不得藏在 profile 里）"
    } else {
        ""
    };
    format!("解析 profiles 文件失败 {}：{detail}{hint}", path.display())
}

fn validate(file: &ProfilesFile) -> Result<(), CtlError> {
    for (name, profile) in &file.profiles {
        if let Some(level) = profile.permission_level.as_deref()
            && !PERMISSION_LEVELS.contains(&level)
        {
            return Err(CtlError::usage(
                "bad_profile_entry",
                format!(
                    "预设 `{name}` 的 permission_level = `{level}` 非法；合法值：{}",
                    PERMISSION_LEVELS.join(" / ")
                ),
            ));
        }
        if let Some(cmds) = &profile.allowed_commands
            && cmds.iter().any(|c| c.trim().is_empty())
        {
            return Err(CtlError::usage(
                "bad_profile_entry",
                format!("预设 `{name}` 的 allowed_commands 含空项（空串会命中所有命令）"),
            ));
        }
        // `*` 是全放行哨兵，等价 `--yes`：藏进预设等于让「读一个 TOML」变成提权
        if let Some(cmds) = &profile.allowed_commands
            && cmds.iter().any(|c| c.trim() == "*")
        {
            return Err(CtlError::usage(
                "bad_profile_entry",
                format!(
                    "预设 `{name}` 的 allowed_commands 含 `*`：等价于 `--yes`，必须由实例级显式决定"
                ),
            ));
        }
    }
    Ok(())
}

impl Profile {
    /// 转成可执行参数：派生键 → `profile.env` → `instance.env`（后者最优先）。
    fn into_resolved(self, instance_env: &BTreeMap<String, String>) -> Resolved {
        let mut env = BTreeMap::new();
        if let Some(cmds) = &self.allowed_commands {
            env.insert("CM_ALLOWED_COMMANDS".to_string(), cmds.join(","));
        }
        if let Some(level) = &self.permission_level {
            env.insert("CM_PERMISSION_LEVEL".to_string(), level.clone());
        }
        if let Some(model) = &self.llm_model {
            env.insert("CM_LLM_MODEL".to_string(), model.clone());
        }
        if let Some(base) = &self.llm_api_base {
            env.insert("CM_LLM_API_BASE".to_string(), base.clone());
        }
        for (k, v) in &self.env {
            env.insert(k.clone(), v.clone());
        }
        for (k, v) in instance_env {
            env.insert(k.clone(), v.clone());
        }
        Resolved {
            serve_config: self.config,
            env,
            no_tools: self.no_tools.unwrap_or(false),
        }
    }
}

/// 按名字取预设并与实例 env 合并；未定义时报 `profile_not_found`。
pub fn resolve(
    file: &ProfilesFile,
    name: &str,
    instance_env: &BTreeMap<String, String>,
    path: &Path,
) -> Result<Resolved, CtlError> {
    let Some(profile) = file.profiles.get(name) else {
        let known: Vec<&str> = file.profiles.keys().map(String::as_str).collect();
        return Err(CtlError::usage(
            "profile_not_found",
            format!(
                "引用不存在的 profile `{name}`（在 {} 中查找；已定义：{}）",
                path.display(),
                if known.is_empty() {
                    "无".to_string()
                } else {
                    known.join(", ")
                }
            ),
        ));
    };
    Ok(profile.clone().into_resolved(instance_env))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cm_ctl_profiles_{tag}_{}_{}",
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
        std::fs::write(path, body).expect("写 profiles");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("设权限");
    }

    fn parse(body: &str) -> Result<ProfilesFile, CtlError> {
        let dir = tmpdir("parse");
        let p = dir.join(FILE_NAME);
        write_with_mode(&p, body, 0o600);
        let out = load_optional(&p);
        let _ = std::fs::remove_dir_all(&dir);
        out
    }

    const FULL: &str = r#"
[profiles.audit]
config = "/tmp/base.toml"
allowed_commands = ["grep", "cat"]
permission_level = "read"
no_tools = true
llm_model = "deepseek-chat"
llm_api_base = "http://127.0.0.1:11434/v1"

[profiles.audit.env]
GIT_SSH_COMMAND = "ssh -i k"
"#;

    #[test]
    fn loads_full_profile() {
        let f = parse(FULL).expect("应加载成功");
        let p = f.profiles.get("audit").expect("应有 audit");
        assert_eq!(p.config.as_deref(), Some("/tmp/base.toml"));
        assert_eq!(p.permission_level.as_deref(), Some("read"));
        assert_eq!(p.no_tools, Some(true));
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let dir = tmpdir("absent");
        let f = load_optional(&dir.join(FILE_NAME)).expect("缺失应视为空");
        assert!(f.profiles.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_world_writable_file() {
        let dir = tmpdir("insecure");
        let p = dir.join(FILE_NAME);
        write_with_mode(&p, FULL, 0o644);
        let err = load_optional(&p).expect_err("0644 必须拒绝");
        assert_eq!(err.error_code, "profiles_file_insecure");
        assert!(err.message.contains("chmod 600"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_workspace_inside_profile() {
        let err = parse("[profiles.a]\nworkspace = \"/tmp/x\"\n").expect_err("workspace 必须拒绝");
        assert_eq!(err.error_code, "profiles_file_invalid");
        assert!(err.message.contains("只能由实例声明提供"), "{}", err.message);
    }

    #[test]
    fn rejects_yes_inside_profile() {
        let err = parse("[profiles.a]\nyes = true\n").expect_err("yes 必须拒绝");
        assert!(err.message.contains("实例级显式决定"), "{}", err.message);
    }

    #[test]
    fn rejects_unknown_key() {
        let err = parse("[profiles.a]\nallowed_command = []\n").expect_err("未知键必须报错");
        assert_eq!(err.error_code, "profiles_file_invalid");
    }

    #[test]
    fn rejects_illegal_permission_level() {
        // 常见误写：readonly 不是合法等级（合法的是 read）
        let err = parse("[profiles.a]\npermission_level = \"readonly\"\n").expect_err("应拒绝");
        assert_eq!(err.error_code, "bad_profile_entry");
        assert!(err.message.contains("chat / read"), "{}", err.message);
    }

    #[test]
    fn rejects_empty_allowed_command() {
        let err = parse("[profiles.a]\nallowed_commands = [\"grep\", \"\"]\n").expect_err("应拒绝");
        assert_eq!(err.error_code, "bad_profile_entry");
    }

    #[test]
    fn merge_order_prefers_instance_env_over_profile() {
        let f = parse(FULL).expect("加载");
        let mut instance_env = BTreeMap::new();
        instance_env.insert("CM_LLM_MODEL".to_string(), "from-instance".to_string());

        let r = resolve(&f, "audit", &instance_env, Path::new("/tmp/profiles.toml")).expect("解析");
        assert_eq!(
            r.env.get("CM_LLM_MODEL").map(String::as_str),
            Some("from-instance"),
            "instance.env 必须覆盖 profile"
        );
        assert_eq!(
            r.env.get("CM_ALLOWED_COMMANDS").map(String::as_str),
            Some("grep,cat"),
            "allowed_commands 派生为 CSV"
        );
        assert_eq!(r.env.get("CM_PERMISSION_LEVEL").map(String::as_str), Some("read"));
        assert_eq!(r.env.get("CM_LLM_API_BASE").map(String::as_str), Some("http://127.0.0.1:11434/v1"));
        assert_eq!(r.env.get("GIT_SSH_COMMAND").map(String::as_str), Some("ssh -i k"));
        assert!(r.no_tools);
        assert_eq!(r.serve_config.as_deref(), Some("/tmp/base.toml"));
    }

    #[test]
    fn profile_env_overrides_derived_keys() {
        let body = r#"
[profiles.a]
allowed_commands = ["grep"]

[profiles.a.env]
CM_ALLOWED_COMMANDS = "cat"
"#;
        let f = parse(body).expect("加载");
        let r = resolve(&f, "a", &BTreeMap::new(), Path::new("/tmp/p.toml")).expect("解析");
        assert_eq!(
            r.env.get("CM_ALLOWED_COMMANDS").map(String::as_str),
            Some("cat"),
            "profile.env 应覆盖派生键"
        );
    }

    #[test]
    fn fingerprint_is_stable_and_tracks_every_field() {
        let base = Resolved {
            serve_config: Some("/tmp/c.toml".to_string()),
            env: BTreeMap::from([("CM_ALLOWED_COMMANDS".to_string(), "grep".to_string())]),
            no_tools: false,
        };
        // 相同内容 ⇒ 相同指纹（否则每次收敛都会误判为变更而重启）
        assert_eq!(base.fingerprint(), base.clone().fingerprint());

        let mut changed_env = base.clone();
        changed_env
            .env
            .insert("CM_ALLOWED_COMMANDS".to_string(), "grep,cat".to_string());
        let mut changed_tools = base.clone();
        changed_tools.no_tools = true;
        let mut changed_config = base.clone();
        changed_config.serve_config = None;

        for other in [changed_env, changed_tools, changed_config] {
            assert_ne!(base.fingerprint(), other.fingerprint());
        }
    }

    #[test]
    fn unknown_profile_reports_lookup_path() {
        let f = parse(FULL).expect("加载");
        let err = resolve(&f, "nope", &BTreeMap::new(), Path::new("/tmp/profiles.toml"))
            .expect_err("未定义应报错");
        assert_eq!(err.error_code, "profile_not_found");
        assert!(err.message.contains("/tmp/profiles.toml"), "{}", err.message);
        assert!(err.message.contains("audit"), "{}", err.message);
    }
}

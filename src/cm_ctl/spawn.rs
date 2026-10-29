//! 子进程构造：**全仓唯一**允许出现 `Command` 的地方。
//!
//! 三条硬约束：
//!
//! 1. **环境不继承**：`env_clear()` 后只注入白名单 + 声明值。否则 ctl 自身的
//!    `CM_*` / `API_KEY` 会静默覆盖子实例配置，形成「看起来隔离、实际同源」；
//! 2. **独立 cwd**：`serve` 的会话库与 `.serve.lock` 按 **cwd** 相对解析，同 cwd
//!    起第二个实例会直接启动失败，故每实例一个专属目录；
//! 3. **自成进程组**：`process_group(0)`（无 `unsafe`），`down` 才能整组回收工具子进程。

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::CtlError;
use super::paths;

/// 默认透传的环境变量（进程运行必需 + 代理/CA 这类高频漏项）。
///
/// 只列**名**；是否真的存在于 ctl 环境中由 [`build_env`] 决定。
const BASE_ENV_KEYS: &[&str] = &[
    "PATH",
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_STATE_HOME",
    "XDG_RUNTIME_DIR",
    "XDG_CACHE_HOME",
    // 走代理 / 私有 CA 的环境缺了这几个会以 DNS/TLS 报错失败，且报错完全不指向 ctl
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// 允许由**声明**显式给出的 `CM_*` 键（`profiles.toml` 的预设映射到这些）。
///
/// 只放行这几个具体键，**不做前缀放行**：`CM_` 前缀放行等于允许注入
/// `CM_SERVE_DISABLE_INSTANCE_LOCK` 这类变量，直接打掉实例隔离。
/// 注意语义是「声明给出」而非「继承」——这些键不在 [`BASE_ENV_KEYS`] 里，
/// 不可能从 ctl 自身环境漏进子进程。
const DECLARABLE_CM_KEYS: &[&str] = &[
    "CM_ALLOWED_COMMANDS",
    "CM_PERMISSION_LEVEL",
    "CM_LLM_MODEL",
    "CM_LLM_API_BASE",
];

/// 永不透传的变量：一旦继承会让「隔离」形同虚设。
fn is_forbidden(key: &str) -> bool {
    if DECLARABLE_CM_KEYS.contains(&key) {
        return false;
    }
    key == "API_KEY"
        || key == "CM_SERVE_DISABLE_INSTANCE_LOCK"
        || key.starts_with("CM_")
        || key.ends_with("_TOKEN")
        || key.ends_with("_SECRET")
}

/// 构造子进程环境：白名单 ∩ 当前环境 + 声明值（声明值优先）。
///
/// 返回 `(环境变量, 实际注入的变量名)`；变量名落进状态文件的 `env_keys`，
/// 供用户核对「有没有发生隐式继承」。
pub fn build_env(
    declared: &BTreeMap<String, String>,
) -> Result<(BTreeMap<String, String>, Vec<String>), CtlError> {
    let mut env = BTreeMap::new();

    for key in BASE_ENV_KEYS {
        if let Ok(val) = std::env::var(key) {
            env.insert((*key).to_string(), val);
        }
    }

    for (key, val) in declared {
        if is_forbidden(key) {
            return Err(CtlError::usage(
                "forbidden_env",
                format!("拒绝透传敏感环境变量 `{key}`（实例隔离要求不继承 CM_*/API_KEY/*_TOKEN/*_SECRET）"),
            ));
        }
        env.insert(key.clone(), val.clone());
    }

    let keys: Vec<String> = env.keys().cloned().collect();
    Ok((env, keys))
}

/// 一次 spawn 所需的全部受控输入（只由显式声明构造，不接受任意 argv）。
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// 工作区；`None` = **待绑定**（子 serve 以 `--workspace-unset` 启动，等 client
    /// 通过 `POST /workspace` 设置）。见 Issue #1183。
    pub workspace: Option<PathBuf>,
    /// 交给子 serve 的 `--config`；`None` 表示让 serve 按自身 XDG 顺序解析。
    ///
    /// 相对路径在 spawn 时按 **ctl 的 cwd** 绝对化后再交给子进程：子进程 cwd 是
    /// 实例目录，原样传 `rel.toml` 会解析到 `<实例目录>/rel.toml`。
    pub serve_config: Option<PathBuf>,
    pub port: u16,
    /// `--yes`（跳过命令白名单）：必须由实例声明显式给出，绝不隐式继承。
    pub yes: bool,
    /// `--no-tools`（禁用全部工具）：来自 profile 的 `no_tools`
    pub no_tools: bool,
    pub env: BTreeMap<String, String>,
    pub log_path: PathBuf,
    /// 子进程 cwd（实例专属目录）。
    pub home: PathBuf,
    pub ready_json: bool,
    /// `true`：stdout/stderr 继承给调用方（`ctl run` 前台模式，便于 systemd/docker 采集）；
    /// `false`：重定向到 `log_path`（`ctl up` 脱管模式）。
    pub inherit_io: bool,
    /// `true`：自成进程组（`ctl up` 脱管，`down` 才能整组回收）；
    /// `false`：留在 ctl 的进程组（`ctl run` 前台，Ctrl-C/SIGTERM 天然同时到达两者，
    /// 比手写信号转发更简单也更符合 Unix 直觉）。
    pub own_process_group: bool,
}

/// spawn 结果。
pub struct Spawned {
    pub child: std::process::Child,
    /// 实际注入的环境变量名（写入 `env_keys`）。
    pub env_keys: Vec<String>,
}

impl Spawned {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

fn build_args(spec: &SpawnSpec, serve_config: Option<&Path>) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    // `--no-tools` 是 clap 的 **global** 选项，文档约定写在子命令之前
    if spec.no_tools {
        args.push("--no-tools".to_string());
    }
    args.push("serve".to_string());
    args.push("--port".to_string());
    args.push(spec.port.to_string());
    // 工作区二选一，不能都不给：`--workspace-unset` 是「先起来、等 client 绑定」的显式声明，
    // 与「忘了传 --workspace」在语义上不同（后者会让 serve 回落到配置的默认目录）。
    match &spec.workspace {
        Some(ws) => {
            args.push("--workspace".to_string());
            args.push(ws.display().to_string());
        }
        None => args.push("--workspace-unset".to_string()),
    }
    if let Some(cfg) = serve_config {
        args.push("--config".to_string());
        args.push(cfg.display().to_string());
    }
    if spec.yes {
        args.push("--yes".to_string());
    }
    if spec.ready_json {
        args.push("--web-ready-json".to_string());
    }
    args
}

fn open_log(path: &Path) -> Result<File, CtlError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            CtlError::general(
                "log_unwritable",
                format!("创建日志目录失败 {}：{e}", dir.display()),
            )
        })?;
    }
    let f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| {
            CtlError::general(
                "log_unwritable",
                format!("打开日志文件失败 {}：{e}", path.display()),
            )
        })?;
    // 日志含 workspace 路径与启动自检信息：收紧到仅本人可读写
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| {
                CtlError::general(
                    "log_unwritable",
                    format!("设置日志权限 0600 失败 {}：{e}", path.display()),
                )
            })?;
    }
    Ok(f)
}

/// 定位 `crabmate`（**不是** `crabmatectl`）可执行文件。
///
/// 不能用 `current_exe()`：`crabmatectl` 是独立二进制，拿它当子进程会得到
/// 「unrecognized subcommand 'serve'」。发行包（tar / deb）把两者放在同一 bin 目录，
/// 故优先取同目录的 `crabmate`；自定义布局用 `CRABMATECTL_BIN` 覆盖。
fn resolve_serve_binary() -> Result<PathBuf, CtlError> {
    if let Some(raw) = std::env::var_os("CRABMATECTL_BIN").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(raw);
        if path.is_file() {
            return Ok(path);
        }
        return Err(CtlError::general(
            "serve_binary_missing",
            format!("CRABMATECTL_BIN 指向的文件不存在：{}", path.display()),
        ));
    }
    let exe = std::env::current_exe().map_err(|e| {
        CtlError::general("spawn_failed", format!("无法定位当前可执行文件：{e}"))
    })?;
    let sibling = exe.with_file_name("crabmate");
    if sibling.is_file() {
        return Ok(sibling);
    }
    Err(CtlError::general(
        "serve_binary_missing",
        format!(
            "找不到 crabmate 可执行文件（期望同目录下的 {}）；请把 crabmate 与 crabmatectl 放在一起，或用 CRABMATECTL_BIN 指定",
            sibling.display()
        ),
    ))
}

/// 子进程的 stdout/stderr：前台模式继承调用方，脱管模式重定向到实例日志。
///
/// 与 [`resolve_serve_config`] 同理独立成函数，替 `spawn_serve` 守住 CCN ≤ 10。
fn open_stdio(spec: &SpawnSpec) -> Result<(Stdio, Stdio), CtlError> {
    if spec.inherit_io {
        return Ok((Stdio::inherit(), Stdio::inherit()));
    }
    let log = open_log(&spec.log_path)?;
    let log_err = log.try_clone().map_err(|e| {
        CtlError::general("log_unwritable", format!("复制日志句柄失败：{e}"))
    })?;
    Ok((Stdio::from(log), Stdio::from(log_err)))
}

/// 交给子进程的 `--config`：相对路径按 **ctl 的 cwd** 绝对化（子进程 cwd 是实例目录）。
///
/// 独立成函数而非内联：`spawn_serve` 已经装满早退分支，内联这层 `match` 会把它的
/// CCN 推过 10（仓库上限）。
fn resolve_serve_config(spec: &SpawnSpec) -> Result<Option<PathBuf>, CtlError> {
    match spec.serve_config.as_deref() {
        Some(p) => Ok(Some(paths::absolutize(&p.to_string_lossy())?)),
        None => Ok(None),
    }
}

/// 启动一个 `serve` 子进程（脱管：ctl 退出后继续运行）。
pub fn spawn_serve(spec: &SpawnSpec) -> Result<Spawned, CtlError> {
    let exe = resolve_serve_binary()?;
    let (env, env_keys) = build_env(&spec.env)?;
    let serve_config = resolve_serve_config(spec)?;
    let (stdout, stderr) = open_stdio(spec)?;

    std::fs::create_dir_all(&spec.home).map_err(|e| {
        CtlError::general(
            "state_dir_unwritable",
            format!("创建实例目录失败 {}：{e}", spec.home.display()),
        )
    })?;
    // 实例目录即子进程 cwd：会话库、上传目录都落在其中，按 0700 收紧
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&spec.home, std::fs::Permissions::from_mode(0o700)).map_err(
            |e| {
                CtlError::general(
                    "state_dir_unwritable",
                    format!("设置实例目录权限 0700 失败 {}：{e}", spec.home.display()),
                )
            },
        )?;
    }

    let mut cmd = Command::new(exe);
    cmd.args(build_args(spec, serve_config.as_deref()))
        .current_dir(&spec.home)
        .env_clear()
        .envs(&env)
        // 子进程不读终端输入，避免抢占调用方的 stdin
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);

    #[cfg(unix)]
    if spec.own_process_group {
        // 自成进程组（无 unsafe；`setsid` 需要 pre_exec + libc，而实测 PTY 关闭时
        // 只要实例已不是 shell 的作业就不会被 SIGHUP 带走，故此已足够）。
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let child = cmd.spawn().map_err(|e| {
        CtlError::general(
            "spawn_failed",
            format!("启动 serve 失败（{} {:?}）：{e}", spec.port, spec.workspace),
        )
    })?;

    Ok(Spawned { child, env_keys })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_includes_serve_and_controlled_flags_only() {
        let spec = SpawnSpec {
            workspace: Some(PathBuf::from("/w")),
            serve_config: Some(PathBuf::from("/c.toml")),
            port: 8080,
            yes: true,
            no_tools: true,
            env: BTreeMap::new(),
            log_path: PathBuf::from("/tmp/x.log"),
            home: PathBuf::from("/tmp/x"),
            ready_json: true,
            inherit_io: false,
            own_process_group: true,
        };
        let args = build_args(&spec, spec.serve_config.as_deref());
        // `--no-tools` 是 clap global 选项，按文档约定前置于子命令
        assert_eq!(args[0], "--no-tools");
        assert_eq!(args[1], "serve");
        assert!(args.windows(2).any(|w| w == ["--port", "8080"]));
        assert!(args.windows(2).any(|w| w == ["--workspace", "/w"]));
        assert!(args.windows(2).any(|w| w == ["--config", "/c.toml"]));
        assert!(args.iter().any(|a| a == "--yes"));
        assert!(args.iter().any(|a| a == "--no-tools"));
        assert!(args.iter().any(|a| a == "--web-ready-json"));
    }

    #[test]
    fn build_args_omits_optional_flags_when_unset() {
        let spec = SpawnSpec {
            workspace: Some(PathBuf::from("/w")),
            serve_config: None,
            port: 8081,
            yes: false,
            no_tools: false,
            env: BTreeMap::new(),
            log_path: PathBuf::from("/tmp/x.log"),
            home: PathBuf::from("/tmp/x"),
            ready_json: false,
            inherit_io: false,
            own_process_group: true,
        };
        let args = build_args(&spec, spec.serve_config.as_deref());
        assert!(!args.iter().any(|a| a == "--config"), "不应隐式传 --config");
        assert!(!args.iter().any(|a| a == "--yes"), "不应隐式开 --yes");
        assert!(!args.iter().any(|a| a == "--no-tools"), "不应隐式开 --no-tools");
    }

    #[test]
    fn env_never_inherits_ctl_secrets() {
        // 白名单里根本没有 API_KEY / CM_*，因此构造结果不可能含它们。
        // （不修改进程环境：那既需 unsafe，也会与并行测试互相干扰）
        let (env, keys) = build_env(&BTreeMap::new()).expect("构造环境应成功");
        assert!(!env.contains_key("API_KEY"), "API_KEY 不得透传");
        assert!(!env.contains_key("CM_LLM_MODEL"), "CM_* 不得透传");
        assert!(
            !keys.iter().any(|k| k.starts_with("CM_")),
            "env_keys 不得出现 CM_*：{keys:?}"
        );
        assert!(!keys.iter().any(|k| k == "API_KEY"));
    }

    #[test]
    fn is_forbidden_matches_sensitive_shapes() {
        for k in [
            "API_KEY",
            "CM_SERVE_DISABLE_INSTANCE_LOCK",
            "CM_WEB_API_BEARER_TOKEN",
            "CM_ANYTHING_ELSE",
            "GITHUB_TOKEN",
            "MY_SECRET",
        ] {
            assert!(is_forbidden(k), "{k} 应被禁止透传");
        }
        // 声明可给的受控 CM_ 键与普通变量
        for k in [
            "PATH",
            "HOME",
            "HTTPS_PROXY",
            "GIT_SSH_COMMAND",
            "CM_ALLOWED_COMMANDS",
            "CM_PERMISSION_LEVEL",
            "CM_LLM_MODEL",
            "CM_LLM_API_BASE",
        ] {
            assert!(!is_forbidden(k), "{k} 应允许");
        }
    }

    #[test]
    fn declared_controlled_cm_key_is_allowed() {
        // `profiles.toml` 的预设就靠这条通道落地（allowed_commands / permission_level / llm_*）
        let mut declared = BTreeMap::new();
        declared.insert("CM_LLM_MODEL".to_string(), "deepseek-chat".to_string());
        let (env, keys) = build_env(&declared).expect("受控 CM_ 键应允许");
        assert_eq!(
            env.get("CM_LLM_MODEL").map(String::as_str),
            Some("deepseek-chat")
        );
        assert!(keys.iter().any(|k| k == "CM_LLM_MODEL"));
    }

    #[test]
    fn declared_unlisted_cm_key_is_rejected() {
        // 白名单**外**的 CM_ 键仍必须拒绝：前缀放行会打开实例隔离的缺口
        let mut declared = BTreeMap::new();
        declared.insert("CM_WEB_WORKSPACE_POOL".to_string(), "/".to_string());
        let err = build_env(&declared).expect_err("白名单外的 CM_ 键应拒绝");
        assert_eq!(err.error_code, "forbidden_env");
    }

    #[test]
    fn declared_non_sensitive_env_passes() {
        let mut declared = BTreeMap::new();
        declared.insert("GIT_SSH_COMMAND".to_string(), "ssh -i k".to_string());
        let (env, keys) = build_env(&declared).expect("普通变量应允许");
        assert_eq!(env.get("GIT_SSH_COMMAND").map(String::as_str), Some("ssh -i k"));
        assert!(keys.iter().any(|k| k == "GIT_SSH_COMMAND"));
    }

    #[test]
    fn forbidden_env_rejects_token_and_secret_suffixes() {
        for k in ["GITHUB_TOKEN", "MY_SECRET"] {
            let mut d = BTreeMap::new();
            d.insert(k.to_string(), "v".to_string());
            let err = build_env(&d).expect_err("应拒绝敏感后缀");
            assert_eq!(err.error_code, "forbidden_env");
        }
    }

    #[test]
    fn forbidden_env_rejects_instance_lock_bypass() {
        // 绕过 serve 实例锁会让两个实例并发写同一 SQLite
        let mut d = BTreeMap::new();
        d.insert("CM_SERVE_DISABLE_INSTANCE_LOCK".to_string(), "1".to_string());
        assert!(build_env(&d).is_err());
    }
}

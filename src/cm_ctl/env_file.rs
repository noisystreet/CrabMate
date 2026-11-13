//! 实例凭据通道：把 `env_file` 里的 `KEY=VALUE` 注入子 `serve`。
//!
//! # 为什么是独立通道
//!
//! [`super::spawn::build_env`] 对声明 `env` 一律拒绝 `API_KEY` / `*_TOKEN` /
//! `*_SECRET`：那是 `env_clear()` 之后「不许把 ctl 自己的凭据漏给子进程」的闸门，
//! 放宽它等于让隔离名存实亡。但常驻实例又确实需要一个凭据来源（无人值守时没人
//! 会每轮请求带 `client_llm.api_key`），于是凭据走这里：**只读一个 0600 文件**。
//!
//! # 四条边界（实现即约束，改这里先读 `docs/命令行与路由.md` 关键语义 17）
//!
//! 1. 值**不落状态记录、不进日志**：本模块只把解析结果交给调用方，
//!    `record.env_keys` 仍然只收变量**名**。错误信息里也绝不回显值——
//!    行号 + 键名足够定位，回显整行等于把密钥写进 `last_error`；
//! 2. `CM_*` 照旧拒绝：否则「一个文件」就能改配置面，`env_clear()` 的意义被抹掉；
//! 3. 拒绝进程注入型键（`LD_PRELOAD` / `BASH_ENV` …）：它们能把代码塞进子进程，
//!    已经不是「配置」而是执行面；
//! 4. 权限过宽 / 属主不符 → 直接报错，**不静默降级**：复用
//!    [`super::paths::check_private_file`]，与 `instances.toml` / `profiles.toml` 同一道闸。
//!
//! 允许 `API_KEY` / `*_TOKEN` / `*_SECRET` —— 那正是本通道存在的理由。

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use super::CtlError;
use super::paths;

/// 凭据通道的容器：值可能含 `API_KEY`，所以 `Debug` **只打键数**。
///
/// `SpawnSpec` / `StartSpec` / `Step` 都 `derive(Debug)`，其中任何一个被 `{:?}`
/// 打印（日志、测试失败信息、将来的 `tracing::debug!`）都会把密钥带出去。
/// 把凭据收进这个类型，等于**从类型上**保证「怎么 Debug 都看不到值」。
#[derive(Clone, Default, PartialEq, Eq)]
pub struct CredEnv(BTreeMap<String, String>);

impl CredEnv {
    pub(crate) fn new(inner: BTreeMap<String, String>) -> Self {
        Self(inner)
    }

    /// 供测试断言用：生产路径只需要 [`Self::iter`]。
    #[cfg(test)]
    pub(crate) fn get(&self, key: &str) -> Option<&String> {
        self.0.get(key)
    }

    /// 供测试构造用：生产路径只经 [`Self::new`] / [`Self::extend`] 得到凭据。
    #[cfg(test)]
    pub(crate) fn insert(&mut self, key: String, value: String) {
        self.0.insert(key, value);
    }

    pub(crate) fn extend(&mut self, more: BTreeMap<String, String>) {
        self.0.extend(more);
    }

    /// 取出内部映射：只给「把两份凭据合并成一份」这种调用点用。
    pub(crate) fn into_inner(self) -> BTreeMap<String, String> {
        self.0
    }

    /// 展开成 `Command::env` 需要的形状（只在 [`super::spawn::build_env`] 里调用）。
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.0.iter()
    }
}

impl fmt::Debug for CredEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 连键名都不打：名字本身就能泄露「用了哪家云、开了哪些集成」
        write!(f, "CredEnv(<{} keys hidden>)", self.0.len())
    }
}

/// 进程注入型键：它们能把代码塞进子进程，效果等价于「在我们自己的 spawn 里跑任意代码」，
/// 与「注入一个凭据」完全是两件事。
const INJECTION_KEYS: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "BASH_ENV",
    "ENV",
    "SHELLOPTS",
    "PYTHONSTARTUP",
    "NODE_OPTIONS",
    "PERL5OPT",
    "RUBYOPT",
];

/// 键是否属于「配置面 / 注入面」（不允许经凭据通道进入）。
///
/// 配置面 = `CM_*` + [`super::spawn::BASE_ENV_KEYS`]（`PATH` / `HOME` / `XDG_*` / 代理 /
/// `SSL_CERT_*`）：这些决定子 serve 怎么被拉起来（落盘位置、流量出口、可执行文件解析），
/// 属 ctl 的配置面，不该有第二条通道。白名单**引用** `spawn` 那份常量而不是各写一份，
/// 否则将来新增一个继承键就会在这里静默漏掉。
fn is_forbidden_key(key: &str) -> bool {
    key.starts_with("CM_")
        || super::spawn::BASE_ENV_KEYS.contains(&key)
        || INJECTION_KEYS.contains(&key)
}

/// `KEY` 的合法形状：`[A-Za-z_][A-Za-z0-9_]*`。
///
/// 不做前缀放行、也不接受 `-` / 空格：`Command::env` 会原样接收任何字符串，
/// 而 `env_file` 是人手写的，拼错的键只会表现成「配了却读不到」。
fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 读取并校验一个 `env_file`。
///
/// `path` 必须已由调用方绝对化（`~` 展开 + 相对 ctl cwd）。
pub(crate) fn load(path: &Path) -> Result<CredEnv, CtlError> {
    match std::fs::metadata(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CtlError::general(
                "env_file_missing",
                format!(
                    "声明的 env_file 不存在：{}；请创建它并 `chmod 600`（内容是 `KEY=VALUE` 逐行）",
                    path.display()
                ),
            ));
        }
        Err(e) => {
            return Err(CtlError::general(
                "env_file_unreadable",
                format!("读取 env_file 元数据失败 {}：{e}", path.display()),
            ));
        }
    }
    paths::check_private_file(
        path,
        "凭据文件（env_file）",
        "env_file_insecure",
        "env_file_foreign_owner",
    )?;
    let text = std::fs::read_to_string(path).map_err(|e| {
        CtlError::general(
            "env_file_unreadable",
            format!("读取 env_file 失败 {}：{e}", path.display()),
        )
    })?;
    parse(&text, path).map(CredEnv::new)
}

/// 解析 `KEY=VALUE`：空行与 `#` 注释跳过，值原样保留（首个 `=` 之后全部）。
///
/// **不**处理引号、**不**做变量展开、**不**剥 `export`：这三种都会把 shell 语义
/// 引进来，而 shell 语义正是注入面的来源。值要带空格就直接写空格。
///
/// 错误信息只带行号 + 键名/原因，**绝不回显整行**（那一行里就有值）。
fn parse(text: &str, path: &Path) -> Result<BTreeMap<String, String>, CtlError> {
    let mut out = BTreeMap::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lineno = idx + 1;
        let Some((key, value)) = line.split_once('=') else {
            return Err(bad_line(path, lineno, "缺少 `=`（每行必须是 `KEY=VALUE`）"));
        };
        let key = key.trim();
        if key.is_empty() || !is_valid_key(key) {
            return Err(bad_line(
                path,
                lineno,
                "键名非法（只允许 [A-Za-z_][A-Za-z0-9_]*）",
            ));
        }
        if is_forbidden_key(key) {
            return Err(CtlError::general(
                "env_file_forbidden_key",
                format!(
                    "拒绝 env_file {} 第 {lineno} 行的 `{key}`：凭据通道不允许 `CM_*`（配置面）与进程注入型键",
                    path.display()
                ),
            ));
        }
        if out.insert(key.to_string(), value.to_string()).is_some() {
            return Err(bad_line(
                path,
                lineno,
                &format!("键 `{key}` 重复（前一行已给出；不静默覆盖）"),
            ));
        }
    }
    Ok(out)
}

fn bad_line(path: &Path, lineno: usize, reason: &str) -> CtlError {
    CtlError::general(
        "env_file_invalid",
        format!("env_file {} 第 {lineno} 行无法解析：{reason}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_fixtures::tempdir;

    fn write(path: &Path, body: &str, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, body).expect("写 env_file");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("设权限");
    }

    #[test]
    fn loads_pairs_skipping_comments_and_blank_lines() {
        let dir = tempdir("env_ok");
        let p = dir.join("a.env");
        write(
            &p,
            "# 注释\n\nAPI_KEY=sk-fake-not-a-real-key\nGIT_SSH_COMMAND=ssh -i /k\nURL_WITH_EQ=a=b\n",
            0o600,
        );
        let env = load(&p).expect("应能解析");
        assert_eq!(env.get("API_KEY").map(String::as_str), Some("sk-fake-not-a-real-key"));
        // 值里的 `=` 与空格原样保留：第一个 `=` 之后都是值
        assert_eq!(env.get("URL_WITH_EQ").map(String::as_str), Some("a=b"));
        assert_eq!(
            env.get("GIT_SSH_COMMAND").map(String::as_str),
            Some("ssh -i /k")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 权限过宽必须在读之前就被拒绝，而不是「先读进来再说」。
    #[test]
    fn rejects_world_readable_file() {
        let dir = tempdir("env_perm");
        let p = dir.join("b.env");
        write(&p, "API_KEY=x\n", 0o644);
        let err = load(&p).expect_err("0644 应被拒绝");
        assert_eq!(err.error_code, "env_file_insecure");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_reported_separately() {
        let dir = tempdir("env_missing");
        let err = load(&dir.join("nope.env")).expect_err("缺失应报错");
        assert_eq!(err.error_code, "env_file_missing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 配置面继续拒绝：`CM_*` 若能从文件进来，`env_clear()` 就白做了。
    #[test]
    fn rejects_cm_prefixed_keys() {
        let dir = tempdir("env_cm");
        let p = dir.join("c.env");
        write(&p, "CM_ALLOWED_COMMANDS=grep\n", 0o600);
        let err = load(&p).expect_err("CM_* 应被拒绝");
        assert_eq!(err.error_code, "env_file_forbidden_key");
        assert!(err.message.contains("CM_ALLOWED_COMMANDS"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 注入型键：效果是「在子进程里跑任意代码」，不属于凭据。
    #[test]
    fn rejects_injection_keys() {
        for (name, key) in [("ld", "LD_PRELOAD"), ("bash", "BASH_ENV")] {
            let dir = tempdir(&format!("env_inject_{name}"));
            let p = dir.join("d.env");
            write(&p, &format!("{key}=/tmp/evil\n"), 0o600);
            let err = load(&p).expect_err("注入型键应被拒绝");
            assert_eq!(err.error_code, "env_file_forbidden_key");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn rejects_duplicate_key_instead_of_overwriting() {
        let dir = tempdir("env_dup");
        let p = dir.join("e.env");
        write(&p, "API_KEY=a\nAPI_KEY=b\n", 0o600);
        let err = load(&p).expect_err("重复键应报错");
        assert_eq!(err.error_code, "env_file_invalid");
        assert!(err.message.contains("重复"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_malformed_line_and_bad_key() {
        let dir = tempdir("env_bad");
        let p = dir.join("f.env");
        write(&p, "NO_EQUALS_HERE\n", 0o600);
        assert_eq!(
            load(&p).expect_err("缺 = 应报错").error_code,
            "env_file_invalid"
        );
        write(&p, "1BAD=x\n", 0o600);
        assert_eq!(
            load(&p).expect_err("键名非法应报错").error_code,
            "env_file_invalid"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 错误信息不得回显值：那条消息可能进 `last_error` / 终端 / CI 日志。
    #[test]
    fn error_message_never_echoes_the_value() {
        let dir = tempdir("env_noecho");
        let p = dir.join("g.env");
        write(&p, "CM_BAD=sk-super-secret-value\n", 0o600);
        let err = load(&p).expect_err("应被拒绝");
        assert!(
            !err.message.contains("sk-super-secret-value"),
            "错误信息泄露了值：{}",
            err.message
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// CRLF 文件：行尾 `\r` 必须被裁掉，否则密钥会变成 `sk-…\r`，
    /// 症状是「明明设了却 401」，几乎无法排查。
    #[test]
    fn crlf_file_keeps_values_clean() {
        let dir = tempdir("env_crlf");
        let p = dir.join("h.env");
        write(&p, "API_KEY=sk-fake\r\nOTHER=v\r\n", 0o600);
        let env = load(&p).expect("应能解析");
        assert_eq!(env.get("API_KEY").map(String::as_str), Some("sk-fake"));
        assert_eq!(env.get("OTHER").map(String::as_str), Some("v"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 配置面（`XDG_*` / 代理 / `SSL_CERT_*` / `PATH`）与 `CM_*` 一样拒绝：
    /// 它们改的是「子 serve 怎么被拉起来」，不是凭据。
    #[test]
    fn rejects_keys_the_spawn_whitelist_owns() {
        for key in ["XDG_STATE_HOME", "HTTPS_PROXY", "SSL_CERT_FILE", "PATH"] {
            let dir = tempdir(&format!("env_cfg_{}", key.to_lowercase()));
            let p = dir.join("i.env");
            write(&p, &format!("{key}=/somewhere\n"), 0o600);
            let err = load(&p).expect_err("配置面键应被拒绝");
            assert_eq!(err.error_code, "env_file_forbidden_key", "{key}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// `CredEnv` 的 `Debug` 绝不能打出值：`SpawnSpec` / `StartSpec` / `Step` 都 `derive(Debug)`。
    #[test]
    fn debug_never_prints_values_or_keys() {
        let dir = tempdir("env_debug");
        let p = dir.join("j.env");
        write(&p, "API_KEY=sk-super-secret-value\n", 0o600);
        let env = load(&p).expect("应能解析");
        let rendered = format!("{env:?}");
        assert!(!rendered.contains("sk-super-secret-value"), "{rendered}");
        assert!(!rendered.contains("API_KEY"), "连键名也不打：{rendered}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! 用户级配置目录（XDG Config）与从系统包（`/etc/crabmate`）的首次种子拷贝。

use std::fs;
use std::path::{Path, PathBuf};

use super::xdg::{self, ENV_CONFIG_DIR};

/// Debian / 桌面安装布局下的系统配置目录。
pub const SYSTEM_CONFIG_DIR: &str = "/etc/crabmate";

/// 设为 `1` / `true` / `yes` / `on` 时跳过从 `/etc/crabmate` 的种子拷贝（测试/CI 用）。
pub const ENV_SKIP_CONFIG_SEED: &str = "CM_CRABMATE_SKIP_CONFIG_SEED";

pub use super::xdg::user_config_dir;

/// 用户级主配置文件：`…/crabmate/config.toml`。
#[must_use]
pub fn user_config_toml_path() -> PathBuf {
    xdg::user_config_dir().join("config.toml")
}

/// 系统包主配置：`/etc/crabmate/config.toml`。
#[must_use]
pub fn system_config_toml_path() -> PathBuf {
    PathBuf::from(SYSTEM_CONFIG_DIR).join("config.toml")
}

fn env_flag_truthy(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|s| {
            let t = s.trim();
            t == "1"
                || t.eq_ignore_ascii_case("true")
                || t.eq_ignore_ascii_case("yes")
                || t.eq_ignore_ascii_case("on")
        })
        .unwrap_or(false)
}

/// cwd 是否落在 CrabMate 源码树内（向上最多若干级找仓库根标记）。
///
/// 用于开发：`cargo test` / `cargo run` 在仓库内时默认不自动采用本机 XDG 用户配置，
/// 避免装过桌面 deb 后污染嵌入默认与本地调试。显式 **`CM_CRABMATE_CONFIG_DIR`** 时仍走 XDG。
#[must_use]
pub fn cwd_in_crabmate_source_tree() -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    cwd.ancestors().take(8).any(|dir| {
        dir.join("src/cm_config/mod.rs").is_file()
            && dir.join("config/default_config.toml").is_file()
    })
}

/// cwd 是否已有项目级覆盖文件（`config.toml` / `.agent_demo.toml`）。
#[must_use]
pub fn cwd_has_local_user_config() -> bool {
    Path::new("config.toml").is_file() || Path::new(".agent_demo.toml").is_file()
}

/// 无 `--config` 且无 cwd 本地覆盖时，是否应解析本机 XDG 用户配置。
#[must_use]
pub fn should_resolve_xdg_user_config() -> bool {
    if std::env::var(ENV_CONFIG_DIR)
        .ok()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    !cwd_in_crabmate_source_tree()
}

/// 运行时实际会加载的用户级文件（种子时只拷这些；其余 `/etc` 分片仍由嵌入默认提供）。
/// `skills/`：系统包可选自带技能，种子到用户 XDG 后由 `skills_user_dir` 扫描。
const SEED_ROOT_FILES: &[&str] = &["config.toml", "agent_roles.toml"];
const SEED_ROOT_DIRS: &[&str] = &["prompts", "config", "skills"];

/// 若用户 `config.toml` 尚不存在，且 `system_dir/config.toml` 存在，则拷贝运行时所需子集到 `user_dir`（**不覆盖**已有文件）。
///
/// 返回 `Ok(true)` 表示执行了种子拷贝；`Ok(false)` 表示跳过。
pub fn ensure_user_config_seeded_from(system_dir: &Path, user_dir: &Path) -> Result<bool, String> {
    let user_toml = user_dir.join("config.toml");
    if user_toml.is_file() {
        return Ok(false);
    }
    let system_toml = system_dir.join("config.toml");
    if !system_toml.is_file() {
        return Ok(false);
    }
    fs::create_dir_all(user_dir)
        .map_err(|e| format!("无法创建用户配置目录 \"{}\": {e}", user_dir.display()))?;
    copy_seed_subset_no_overwrite(system_dir, user_dir)?;
    if !user_toml.is_file() {
        return Err(format!("种子拷贝后仍缺少 \"{}\"", user_toml.display()));
    }
    Ok(true)
}

/// 从 **`/etc/crabmate`** 种子到 [`user_config_dir`]（仅首次、不覆盖）。
pub fn ensure_user_config_seeded_from_system() -> Result<bool, String> {
    if env_flag_truthy(ENV_SKIP_CONFIG_SEED) {
        return Ok(false);
    }
    ensure_user_config_seeded_from(Path::new(SYSTEM_CONFIG_DIR), &user_config_dir())
}

/// 无 `--config`、无 cwd 本地覆盖、且允许解析 XDG 时：先尝试种子，再返回已存在的用户 `config.toml`。
#[must_use]
pub fn resolve_default_user_config_toml_after_seed() -> Option<PathBuf> {
    if !should_resolve_xdg_user_config() {
        return None;
    }
    if let Err(e) = ensure_user_config_seeded_from_system() {
        log::warn!("seed user config from {SYSTEM_CONFIG_DIR}: {e}");
    }
    let p = user_config_toml_path();
    p.is_file().then_some(p)
}

/// 交互式 CLI（`repl` / `tui` / `chat` / `serve`）配置路径：与桌面 Tauri 对齐。
///
/// - 调用方已有非空 `--config` → 返回 `None`（继续用显式路径）
/// - cwd 已有 `config.toml` / `.agent_demo.toml` → `None`（由 [`crate::cm_config::load_config`] 合并 cwd）
/// - 否则种子后返回用户 XDG `config.toml`（**源码树内也采用**，便于与桌面共用）
/// - 用户副本仍无且 `/etc/crabmate/config.toml` 存在 → 只读回退系统模板
#[must_use]
pub fn resolve_interactive_cli_config_path(explicit: Option<&str>) -> Option<PathBuf> {
    if explicit.map(str::trim).is_some_and(|s| !s.is_empty()) {
        return None;
    }
    if cwd_has_local_user_config() {
        return None;
    }
    let seed = ensure_user_config_seeded_from_system();
    let user = user_config_toml_path();
    if user.is_file() {
        return Some(user);
    }
    if let Err(e) = seed {
        log::warn!("seed user config from {SYSTEM_CONFIG_DIR}: {e}");
    }
    let system = system_config_toml_path();
    system.is_file().then_some(system)
}

fn copy_seed_subset_no_overwrite(src: &Path, dst: &Path) -> Result<(), String> {
    for name in SEED_ROOT_FILES {
        let from = src.join(name);
        if !from.is_file() {
            continue;
        }
        let to = dst.join(name);
        copy_file_no_overwrite(&from, &to)?;
    }
    for name in SEED_ROOT_DIRS {
        let from = src.join(name);
        if !from.is_dir() {
            continue;
        }
        let to = dst.join(name);
        copy_dir_contents_no_overwrite(&from, &to)?;
    }
    Ok(())
}

fn copy_file_no_overwrite(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建 \"{}\": {e}", parent.display()))?;
    }
    fs::copy(from, to).map_err(|e| {
        format!(
            "无法复制 \"{}\" → \"{}\": {e}",
            from.display(),
            to.display()
        )
    })?;
    Ok(())
}

fn copy_dir_entry_no_overwrite(dst: &Path, entry: fs::DirEntry) -> Result<(), String> {
    let file_type = entry
        .file_type()
        .map_err(|e| format!("无法识别 \"{}\": {e}", entry.path().display()))?;
    let from = entry.path();
    let to = dst.join(entry.file_name());
    if file_type.is_dir() {
        copy_dir_contents_no_overwrite(&from, &to)?;
    } else if file_type.is_file() {
        copy_file_no_overwrite(&from, &to)?;
    } else if file_type.is_symlink() && from.is_file() {
        // 跟随为普通文件时再拷；目录 symlink 跳过，避免越出模板树。
        copy_file_no_overwrite(&from, &to)?;
    }
    Ok(())
}

fn copy_dir_contents_no_overwrite(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("无法创建 \"{}\": {e}", dst.display()))?;
    let entries = fs::read_dir(src)
        .map_err(|e| format!("无法读取系统配置目录 \"{}\": {e}", src.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| format!("无法读取系统配置目录 \"{}\": {e}", src.display()))?;
        copy_dir_entry_no_overwrite(dst, entry)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_config::xdg::ENV_CONFIG_DIR;
    use crate::cm_config::xdg::test_env_lock;

    fn recover_cwd_if_needed() {
        if std::env::current_dir().is_err() {
            let fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let _ = std::env::set_current_dir(&fallback);
        }
    }

    #[test]
    fn seed_copies_once_and_never_overwrites() {
        let _guard = test_env_lock();
        recover_cwd_if_needed();
        let tmp = tempfile::tempdir().expect("tempdir");
        let system = tmp.path().join("etc");
        let user = tmp.path().join("xdg");
        fs::create_dir_all(system.join("prompts")).unwrap();
        fs::create_dir_all(system.join("config/prompts")).unwrap();
        fs::write(system.join("config.toml"), b"system-v1\n").unwrap();
        fs::write(system.join("agent_roles.toml"), b"roles-v1\n").unwrap();
        fs::write(system.join("prompts/a.md"), b"prompt\n").unwrap();
        fs::write(system.join("config/prompts/role.md"), b"role\n").unwrap();
        fs::create_dir_all(system.join("skills/packaged")).unwrap();
        fs::write(
            system.join("skills/packaged/SKILL.md"),
            b"---\nname: packaged\n---\nbody\n",
        )
        .unwrap();
        fs::write(system.join("tools.toml"), b"should-not-copy\n").unwrap();
        fs::write(system.join("default_config.toml"), b"should-not-copy\n").unwrap();

        assert!(ensure_user_config_seeded_from(&system, &user).unwrap());
        assert_eq!(
            fs::read_to_string(user.join("config.toml")).unwrap(),
            "system-v1\n"
        );
        assert_eq!(
            fs::read_to_string(user.join("agent_roles.toml")).unwrap(),
            "roles-v1\n"
        );
        assert_eq!(
            fs::read_to_string(user.join("prompts/a.md")).unwrap(),
            "prompt\n"
        );
        assert_eq!(
            fs::read_to_string(user.join("config/prompts/role.md")).unwrap(),
            "role\n"
        );
        assert_eq!(
            fs::read_to_string(user.join("skills/packaged/SKILL.md")).unwrap(),
            "---\nname: packaged\n---\nbody\n"
        );
        assert!(!user.join("tools.toml").exists());
        assert!(!user.join("default_config.toml").exists());

        fs::write(system.join("config.toml"), b"system-v2\n").unwrap();
        fs::write(user.join("config.toml"), b"user-edited\n").unwrap();
        assert!(!ensure_user_config_seeded_from(&system, &user).unwrap());
        assert_eq!(
            fs::read_to_string(user.join("config.toml")).unwrap(),
            "user-edited\n"
        );
    }

    #[test]
    fn seed_skips_when_system_config_missing() {
        let _guard = test_env_lock();
        recover_cwd_if_needed();
        let tmp = tempfile::tempdir().expect("tempdir");
        let system = tmp.path().join("etc");
        let user = tmp.path().join("xdg");
        fs::create_dir_all(&system).unwrap();
        assert!(!ensure_user_config_seeded_from(&system, &user).unwrap());
        assert!(!user.join("config.toml").exists());
    }

    #[test]
    fn skip_seed_env_disables_system_seed() {
        let _guard = test_env_lock();
        recover_cwd_if_needed();
        // SAFETY: serialized by ENV_LOCK; test-only.
        unsafe {
            std::env::set_var(ENV_SKIP_CONFIG_SEED, "1");
        }
        let r = ensure_user_config_seeded_from_system();
        unsafe {
            std::env::remove_var(ENV_SKIP_CONFIG_SEED);
        }
        assert!(!r.unwrap());
    }

    #[test]
    fn source_tree_skips_xdg_unless_config_dir_override() {
        let _guard = test_env_lock();
        recover_cwd_if_needed();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .expect("repo root");
        let prev = std::env::current_dir().unwrap_or_else(|_| root.clone());
        // SAFETY: serialized by ENV_LOCK; test-only.
        unsafe {
            std::env::remove_var(ENV_CONFIG_DIR);
            std::env::remove_var(ENV_SKIP_CONFIG_SEED);
        }
        std::env::set_current_dir(&root).unwrap();
        assert!(
            cwd_in_crabmate_source_tree(),
            "expected repo root markers under {}",
            root.display()
        );
        assert!(
            !should_resolve_xdg_user_config(),
            "source tree must skip XDG without CM_CRABMATE_CONFIG_DIR"
        );
        assert!(
            resolve_default_user_config_toml_after_seed().is_none(),
            "must not resolve/seed XDG inside source tree"
        );

        let fake = tempfile::tempdir().expect("fake xdg");
        unsafe {
            std::env::set_var(ENV_CONFIG_DIR, fake.path());
            std::env::set_var(ENV_SKIP_CONFIG_SEED, "1");
        }
        assert!(should_resolve_xdg_user_config());
        assert!(
            resolve_default_user_config_toml_after_seed().is_none(),
            "empty override dir + skip seed => no config.toml"
        );

        std::env::set_current_dir(prev).unwrap();
        unsafe {
            std::env::remove_var(ENV_CONFIG_DIR);
            std::env::remove_var(ENV_SKIP_CONFIG_SEED);
        }
    }

    #[test]
    fn interactive_cli_resolves_xdg_even_in_source_tree() {
        let _guard = test_env_lock();
        recover_cwd_if_needed();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .expect("repo root");
        let tmp = tempfile::tempdir().expect("tempdir");
        let xdg = tmp.path().join("xdg");
        std::fs::create_dir_all(&xdg).unwrap();
        let user_toml = xdg.join("config.toml");
        std::fs::write(&user_toml, b"[agent]\nmodel = \"xdg\"\n").unwrap();

        let prev = std::env::current_dir().unwrap_or_else(|_| root.clone());
        // SAFETY: serialized by ENV_LOCK; test-only.
        unsafe {
            std::env::set_var(ENV_CONFIG_DIR, &xdg);
            std::env::set_var(ENV_SKIP_CONFIG_SEED, "1");
        }
        std::env::set_current_dir(&root).unwrap();
        assert!(cwd_in_crabmate_source_tree());
        let got = resolve_interactive_cli_config_path(None);
        assert_eq!(got.as_deref(), Some(user_toml.as_path()));
        // 显式 --config 时不改写
        assert!(resolve_interactive_cli_config_path(Some("/tmp/explicit.toml")).is_none());

        std::env::set_current_dir(prev).unwrap();
        unsafe {
            std::env::remove_var(ENV_CONFIG_DIR);
            std::env::remove_var(ENV_SKIP_CONFIG_SEED);
        }
    }
}

//! 写效应名单与归档工具的回归测试。
//!
//! 独立成文件是为了让 `tests.rs` 留在单文件行数上限内：这些用例依赖它的 `test_ctx` /
//! `test_allowed_commands`（已 `pub(super)` 暴露给同层测试模块）。

#[cfg(feature = "archive-tools")]
use super::tests::{test_allowed_commands, test_ctx};
use super::*;

/// 写效应名单（`cm_config::tool_write_effect`）必须与注册工具表对得上。
///
/// 这条测试存在的理由：名单原来由 `cm_tools` 与 `cm_memory` 各维护一份，两份都漂了 ——
/// 一份留着已删除的工具名（死项），另一份漏掉真正写盘的工具（漏项，写操作被当只读）。
/// 现在名单只有一份，但「工具改名 / 删除后忘删名单条目」「新增写工具忘了登记」仍会发生。
#[test]
fn write_effect_list_matches_registered_tools() {
    let registered: std::collections::HashSet<&str> =
        tool_specs().iter().map(|spec| spec.name).collect();
    let listed = crate::cm_config::tool_write_effect::builtin_write_effect_tools();

    let mut dead: Vec<&str> = listed
        .iter()
        .map(String::as_str)
        .filter(|name| !registered.contains(name))
        .collect();
    dead.sort_unstable();
    assert!(
        dead.is_empty(),
        "写效应名单里有未注册的工具名（工具删了/改名了，名单没跟）: {dead:?}"
    );

    // 会写工作区、执行用户代码、或改持久/远端状态的工具：漏一个就意味着它被当只读，
    // 会进并行只读批、只读 TTL / 同轮去重缓存，甚至被 L1 的只读正列表放行。
    for name in [
        "create_file",
        "apply_patch",
        "search_replace",
        "structured",
        "run_command",
        "terminal_session",
        "cargo_test",
        "cargo_run",
        "pytest_run",
        "python_snippet_run",
        "http_request",
        "gh_release_create",
        "long_term_remember",
        "summarize_experience",
        "reminder",
        "event",
        "skill_manage",
    ] {
        assert!(
            listed.contains(name),
            "{name} 会写文件 / 执行代码 / 改持久状态，必须在写效应名单里"
        );
    }

    // `archive_pack` / `archive_unpack` 只在 `archive-tools` feature 下注册，
    // 名单与注册表两侧同时 gate（见 `tool_write_effect` 里的 `#[cfg]`）。
    #[cfg(feature = "archive-tools")]
    for name in ["archive_pack", "archive_unpack"] {
        assert!(
            listed.contains(name),
            "{name} 会往工作区写文件，必须在写效应名单里"
        );
    }

    // 反向：纯读工具不该被误列（列了会让它退出并行只读批与只读缓存）。`cargo_check` /
    // `cargo_clippy` 也只写 `target/` 构建缓存、不改源，口径见 `tool_write_effect` 模块文档。
    for name in [
        "read_file",
        "search_in_files",
        "git",
        "code_nav",
        "archive_list",
        "cargo_check",
    ] {
        assert!(
            !listed.contains(name),
            "{name} 是纯读工具，不该在写效应名单里"
        );
    }
}

/// `archive_*` 迁 Typed 的回归：格式不支持原是无关键词纯文本，会被判成成功。
#[cfg(feature = "archive-tools")]
#[test]
fn archive_tools_report_typed_errors() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.tar.zst"), b"x").expect("write fixture");
    std::fs::write(dir.path().join("a.txt"), b"x").expect("write fixture");
    let access = super::file::PathAccess::new(dir.path());

    let err = super::archive::archive_list(r#"{"archive":"a.tar.zst"}"#, &access, &ctx)
        .expect_err("未知后缀必须报错而不是静默成功");
    assert_eq!(err.code, "invalid_args");
    assert!(
        err.message.contains("不支持的归档格式"),
        "正文应保留原提示: {}",
        err.message
    );

    let err = super::archive::archive_unpack(r#"{"archive":"nope.zip"}"#, &access, &ctx)
        .expect_err("归档不存在必须报错");
    assert_eq!(err.code, "archive_not_found");

    let err = super::archive::archive_pack(r#"{"sources":["a.txt"]}"#, &access, &ctx)
        .expect_err("缺 output 必须报错");
    assert_eq!(err.code, "invalid_args");
}

/// 归档的读写路径都要过 `PathAccess`：`..` 逃出工作区必须被拦，且不留下文件。
#[cfg(feature = "archive-tools")]
#[test]
fn archive_paths_outside_workspace_are_rejected() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.txt"), b"x").expect("write fixture");
    std::fs::write(dir.path().join("a.tar"), b"").expect("write fixture");
    let access = super::file::PathAccess::new(dir.path());

    let err = super::archive::archive_pack(
        r#"{"output":"../escape.tar","sources":["a.txt"]}"#,
        &access,
        &ctx,
    )
    .expect_err("越界 output 必须被拒");
    assert_eq!(err.code, "workspace_resolve_failed");
    assert!(
        !dir.path().join("..").join("escape.tar").exists(),
        "越界路径不得留下文件"
    );

    let err = super::archive::archive_unpack(
        r#"{"archive":"a.tar","output_dir":"../out"}"#,
        &access,
        &ctx,
    )
    .expect_err("越界 output_dir 必须被拒");
    assert_eq!(err.code, "workspace_resolve_failed");

    // `../a.tar` 归一化后落在工作区外，但父目录里没有该文件，`canonicalize` 只会给
    // `NotFound`，与「名字写错」不可区分（存在时才会被 `ensure_canonical_within_root`
    // 判成越界）。故这里用绝对路径钉住越界分支。
    let err = super::archive::archive_list(r#"{"archive":"/etc/passwd"}"#, &access, &ctx)
        .expect_err("绝对路径必须被拒");
    assert_eq!(err.code, "workspace_resolve_failed");
}

//! 从 clap 定义生成 **troff(1)** 手册页：`man/crabmate.1` 与 `man/crabmatectl.1`。
//!
//! 维护者：CLI 有增删时执行
//! `cargo run --features gen-man --bin crabmate-gen-man`
//! 再提交更新后的 `man/*.1`。
//!
//! `clap_mangen` 会为子命令生成 `crabmate\-foo(1)` 式交叉引用，但本仓库**只安装**单页，
//! 故在写入前将子命令标题改为 **粗体名称**（无虚假 `man` 链接）。`crabmatectl` 同理。
//!
//! 两份页分别来自 `crabmate::root_clap_command_for_man_page()` 与
//! `cm_config::cli::ctl::ctl_clap_command_for_man_page()`——都是**当前构建的同一份 clap
//! 定义**，所以手册页不可能与 CLI 漂移。

use std::path::PathBuf;

use crabmate::config::cli::ctl::{SUBCOMMAND_NAMES, ctl_clap_command_for_man_page};

/// `crabmate` 的子命令名，供 troff 交叉引用改写。
///
/// 只能手写：生成器从 clap 拿到的是 `Command`，没有「子命令名是否都已收录」的编译期
/// 提示。新增子命令时记得同步这里——`crabmatectl` 那侧改用了带测试的常量，见
/// [`SUBCOMMAND_NAMES`]。
const CRABMATE_SUBCOMMANDS: &[&str] = &[
    "serve",
    "bench",
    "config",
    "doctor",
    "models",
    "probe",
    "save-session",
    "web-bearer",
    "tool-replay",
    "sse-replay",
    "mcp",
    "plugin",
    "workflow",
    "e2e",
];

/// 把 `parent\-sub(1)` 形式的交叉引用改成粗体子命令名。
///
/// troff 里 `-` 需写成 `\-`，`clap_mangen` 正是按转义形式输出，所以这里两侧都按转义
/// 形式匹配。
fn bold_subcommand_refs(troff: &str, parent: &str, subcommands: &[&str]) -> String {
    let mut out = troff.to_string();
    for sub in subcommands {
        let escaped = sub.replace('-', "\\-");
        out = out.replace(
            &format!("{parent}\\-{escaped}(1)"),
            &format!("\\fB{sub}\\fR"),
        );
    }
    out.replace(
        &format!(
            ".TP\n{parent}\\-help(1)\nPrint this message or the help of the given subcommand(s)\n"
        ),
        &format!(
            ".PP\nUse \\fB{parent} \\-\\-help\\fR or \\fB{parent} \\fIsubcommand\\fR \\fB\\-\\-help\\fR.\n"
        ),
    )
}

/// 生成一页 man 并写入 `man/<file>`，返回写入路径。
fn write_man(
    cmd: clap::Command,
    title: &str,
    manual: &str,
    parent: &str,
    subcommands: &[&str],
    file: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("man");
    std::fs::create_dir_all(&dir)?;
    let man = clap_mangen::Man::new(cmd)
        .title(title)
        .section("1")
        .source(format!("{parent} {}", env!("CARGO_PKG_VERSION")))
        .manual(manual);
    let mut raw = Vec::<u8>::new();
    man.render(&mut raw)?;
    let fixed = bold_subcommand_refs(&String::from_utf8(raw)?, parent, subcommands);
    let path = dir.join(file);
    std::fs::write(&path, fixed.as_bytes())?;
    eprintln!("Wrote {}", path.display());
    Ok(path)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    write_man(
        crabmate::root_clap_command_for_man_page(),
        "CRABMATE",
        "CrabMate User Commands",
        "crabmate",
        CRABMATE_SUBCOMMANDS,
        "crabmate.1",
    )?;
    write_man(
        ctl_clap_command_for_man_page(),
        "CRABMATECTL",
        "CrabMate Local Instance Manager",
        "crabmatectl",
        SUBCOMMAND_NAMES,
        "crabmatectl.1",
    )?;
    Ok(())
}

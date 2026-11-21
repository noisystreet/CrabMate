//! 从 clap 定义生成 **troff(1)** 手册页：`man/crabmate.1` 与 `man/crabmatectl.1`。
//!
//! 维护者：**增删子命令或顶层选项**之后执行
//! `cargo run --features gen-man --bin crabmate-gen-man`
//! 再提交更新后的 `man/*.1`。
//!
//! `--check` 是只校验不写盘的模式：渲染到内存后与磁盘逐字节比较，不一致则报出哪页漂移并以
//! 非零退出。`pre-commit` 的 `check-man-sync` 钩子跑的就是它——这条钩子此前不存在，于是
//! 「加了子命令却没重生成 man」能一路绿到合并（#1338 的 `backfill-layout` 就是这么漏的）。
//!
//! 单页**不含各子命令自己的 flags**——查某个子命令的 flag 用 `<cmd> <sub> --help`。
//! 因此改子命令 flag 不必重生成（重生成也不会产生 diff）；只有顶层选项或子命令增减
//! 才会改变输出。
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
    "backfill-layout",
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

/// 渲染一页 man 的正文（**不写盘**）。
fn render_man(
    cmd: clap::Command,
    title: &str,
    manual: &str,
    parent: &str,
    subcommands: &[&str],
) -> Result<String, Box<dyn std::error::Error>> {
    let man = clap_mangen::Man::new(cmd)
        .title(title)
        .section("1")
        .source(format!("{parent} {}", env!("CARGO_PKG_VERSION")))
        .manual(manual);
    let mut raw = Vec::<u8>::new();
    man.render(&mut raw)?;
    Ok(bold_subcommand_refs(
        &String::from_utf8(raw)?,
        parent,
        subcommands,
    ))
}

fn man_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("man")
}

/// 写盘，或（`check = true`）只比对。返回该页是否一致。
fn emit_man(rendered: &str, file: &str, check: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let dir = man_dir();
    let path = dir.join(file);
    if !check {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&path, rendered.as_bytes())?;
        eprintln!("Wrote {}", path.display());
        return Ok(true);
    }
    // 读不到（文件缺失）也算漂移：`man/` 是版本控制内的产物，不该缺。
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing == rendered {
        return Ok(true);
    }
    eprintln!(
        "man/{file}: 与当前 clap 定义不一致（磁盘 {} 字节 / 渲染 {} 字节）",
        existing.len(),
        rendered.len()
    );
    Ok(false)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let check = std::env::args().skip(1).any(|arg| arg == "--check");

    let crabmate_page = render_man(
        crabmate::root_clap_command_for_man_page(),
        "CRABMATE",
        "CrabMate User Commands",
        "crabmate",
        CRABMATE_SUBCOMMANDS,
    )?;
    let ctl_page = render_man(
        ctl_clap_command_for_man_page(),
        "CRABMATECTL",
        "CrabMate Local Instance Manager",
        "crabmatectl",
        SUBCOMMAND_NAMES,
    )?;

    let mut in_sync = emit_man(&crabmate_page, "crabmate.1", check)?;
    in_sync &= emit_man(&ctl_page, "crabmatectl.1", check)?;

    if !in_sync {
        eprintln!("修复：cargo run --features gen-man --bin crabmate-gen-man（然后提交 man/*.1）");
        std::process::exit(1);
    }
    Ok(())
}

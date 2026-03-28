//! 从 [`crabmate::root_clap_command_for_man_page`] 生成 **troff(1)** 手册页 `man/crabmate.1`。
//!
//! 维护者：CLI 有增删时执行  
//! `cargo run --bin crabmate-gen-man`  
//! 再提交更新后的 `man/crabmate.1`。
//!
//! `clap_mangen` 会为子命令生成 `crabmate\-foo(1)` 式交叉引用，但本仓库**只安装**单页 `crabmate.1`，故在写入前将子命令标题改为 **粗体名称**（无虚假 `man` 链接）。

use std::path::PathBuf;

fn fix_subcommand_crossrefs(troff: &str) -> String {
    let mut out = troff.to_string();
    // clap_mangen 使用「父名-子名(1)」；本包仅分发 crabmate.1。
    let pairs = [
        ("crabmate\\-serve(1)", "\\fBserve\\fR"),
        ("crabmate\\-repl(1)", "\\fBrepl\\fR"),
        ("crabmate\\-chat(1)", "\\fBchat\\fR"),
        ("crabmate\\-bench(1)", "\\fBbench\\fR"),
        ("crabmate\\-config(1)", "\\fBconfig\\fR"),
        ("crabmate\\-doctor(1)", "\\fBdoctor\\fR"),
        ("crabmate\\-models(1)", "\\fBmodels\\fR"),
        ("crabmate\\-probe(1)", "\\fBprobe\\fR"),
        ("crabmate\\-save\\-session(1)", "\\fBsave-session\\fR"),
    ];
    for (from, to) in pairs {
        out = out.replace(from, to);
    }
    out.replace(
        ".TP\ncrabmate\\-help(1)\nPrint this message or the help of the given subcommand(s)\n",
        ".PP\nUse \\fBcrabmate \\-\\-help\\fR or \\fBcrabmate \\fIsubcommand\\fR \\fB\\-\\-help\\fR.\n",
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cmd = crabmate::root_clap_command_for_man_page();
    let man = clap_mangen::Man::new(cmd)
        .title("CRABMATE")
        .section("1")
        .source(format!("crabmate {}", env!("CARGO_PKG_VERSION")))
        .manual("CrabMate User Commands");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("man");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("crabmate.1");
    let mut raw = Vec::<u8>::new();
    man.render(&mut raw)?;
    let fixed = fix_subcommand_crossrefs(&String::from_utf8(raw)?);
    std::fs::write(&path, fixed.as_bytes())?;
    eprintln!("Wrote {}", path.display());
    Ok(())
}

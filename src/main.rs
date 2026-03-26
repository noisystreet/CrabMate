//! CrabMate 可执行文件入口：调用库内 [`crabmate::run`]；[`crabmate::CliExitError`] 映射为约定退出码。

#[tokio::main]
async fn main() {
    if let Err(e) = crabmate::run().await {
        if let Some(cli) = e.downcast_ref::<crabmate::CliExitError>() {
            eprintln!("{}", cli.message);
            std::process::exit(cli.code);
        }
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

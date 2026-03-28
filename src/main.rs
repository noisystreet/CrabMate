//! CrabMate 可执行文件入口：调用库内 [`crabmate::run`]；[`crabmate::CliExitError`] 映射为约定退出码。
//!
//! **`tool-runner-internal`**：Docker 沙盒内由 `docker run … crabmate tool-runner-internal` 调用，非交互；见 [`crabmate::tool_sandbox`]。

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() >= 2 && argv[1] == "tool-runner-internal" {
        if let Err(e) = crabmate::tool_sandbox::tool_runner_internal_main() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tokio runtime：{}", e);
            std::process::exit(1);
        }
    };
    if let Err(e) = rt.block_on(crabmate::run()) {
        if let Some(cli) = e.downcast_ref::<crabmate::CliExitError>() {
            eprintln!("{}", cli.message);
            std::process::exit(cli.code);
        }
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

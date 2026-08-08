//! `crabmate web-bearer`：系统钥匙串中的 Web API 共享密钥（与 Web `/user-data` 同源）。

use std::io::{self, IsTerminal, Read, Write};

use crate::config::cli::WebBearerCli;
use crate::user_data::{read_secret_web_api_bearer, secrets_status, write_secret_web_api_bearer};

const WEB_BEARER_MAX_CHARS: usize = 16384;
const WEB_BEARER_ENV: &str = "CM_WEB_API_BEARER_TOKEN";

/// 执行 `web-bearer status|set|clear`；成功时打印人读摘要（**不**输出明文）。
pub fn run_web_bearer_command(cmd: WebBearerCli) -> Result<(), Box<dyn std::error::Error>> {
    let _ = crate::user_data::ensure_user_data_tree();
    match cmd {
        WebBearerCli::Status => {
            let st = secrets_status();
            if st.web_api_bearer.set {
                println!("web_api_bearer: 已设置（值已隐藏；与 Web/桌面同源）");
            } else {
                println!("web_api_bearer: 未设置");
            }
            println!("详见 docs/design/user_data_dir.md、docs/配置说明.md");
        }
        WebBearerCli::Set {
            token,
            stdin,
            from_env,
        } => {
            let token = resolve_set_token(token, stdin, from_env)?;
            write_secret_web_api_bearer(&token).map_err(std::io::Error::other)?;
            println!(
                "[ok] 已写入系统钥匙串 web_api_bearer（值已隐藏）。TOML / CM_WEB_API_BEARER_TOKEN 皆空时，serve 启动会从此处加载；从无密钥变为有密钥须重启 serve 以挂载鉴权中间件。浏览器设置中仍须保存同一串。"
            );
        }
        WebBearerCli::Clear => {
            write_secret_web_api_bearer("").map_err(std::io::Error::other)?;
            let still = read_secret_web_api_bearer().is_some_and(|s| !s.trim().is_empty());
            if still {
                return Err(std::io::Error::other(
                    "清除后钥匙串仍可读到非空值；请检查系统钥匙串权限后重试。",
                )
                .into());
            }
            println!(
                "[ok] 已清除系统钥匙串 web_api_bearer。若 serve 启动时已挂鉴权中间件，清空后须重启 serve 才会拆除中间件。"
            );
        }
    }
    Ok(())
}

fn resolve_set_token(
    token: Option<String>,
    stdin: bool,
    from_env: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let sources = usize::from(token.as_ref().is_some_and(|s| !s.is_empty()))
        + usize::from(stdin)
        + usize::from(from_env);
    if sources > 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "请只使用一种输入：位置参数 TOKEN、`--stdin` 或 `--from-env`。",
        )
        .into());
    }

    let raw = if from_env {
        std::env::var(WEB_BEARER_ENV).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("环境变量 {WEB_BEARER_ENV} 未设置或不可读。"),
            )
        })?
    } else if stdin {
        read_token_from_stdin()?
    } else if let Some(t) = token.filter(|s| !s.is_empty()) {
        eprintln!(
            "警告: 位置参数 TOKEN 会出现在 shell 历史与进程列表（ps）中；建议改用 `web-bearer set --stdin`、`web-bearer set --from-env`，或无参数时交互隐藏输入。"
        );
        t
    } else {
        read_token_interactively()?
    };

    let token = raw.trim();
    if token.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "token 为空；请提供非空共享密钥，或使用 `web-bearer clear` 清除。",
        )
        .into());
    }
    if token.len() > WEB_BEARER_MAX_CHARS {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("token 过长（上限 {WEB_BEARER_MAX_CHARS} 字符）。"),
        )
        .into());
    }
    Ok(token.to_string())
}

fn read_token_from_stdin() -> Result<String, Box<dyn std::error::Error>> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    // 仅取首行，避免粘贴多行时静默吞掉多余内容却写入整段
    let line = buf.lines().next().unwrap_or("").to_string();
    Ok(line)
}

fn read_token_interactively() -> Result<String, Box<dyn std::error::Error>> {
    if !io::stdin().is_terminal() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "非交互式 stdin：请使用 `web-bearer set --stdin` 或 `web-bearer set --from-env`（勿把密钥写在 argv 上）。",
        )
        .into());
    }
    eprint!("Web API Bearer（输入不回显）: ");
    let _ = io::stderr().flush();
    let term = dialoguer::console::Term::stderr();
    let line = term.read_secure_line().map_err(std::io::Error::other)?;
    eprintln!();
    Ok(line)
}

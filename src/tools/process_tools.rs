//! 进程与端口管理工具（只读）

use std::process::Command;

use super::output_util;

const MAX_OUTPUT_LINES: usize = 400;

pub fn port_check(args_json: &str, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let port = match v.get("port").and_then(|x| x.as_u64()) {
        Some(p) if p > 0 && p <= 65535 => p as u16,
        _ => return "错误：缺少合法 port 参数（1-65535）".to_string(),
    };

    let mut cmd = Command::new("ss");
    cmd.arg("-tlnp").arg(format!("sport = :{}", port));
    match cmd.output() {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if status != 0 && stdout.trim().is_empty() {
                return try_lsof_fallback(port, max_output_len, &stderr);
            }
            let lines: Vec<&str> = stdout.lines().collect();
            if lines.len() <= 1 {
                format!("端口 {} 未被占用", port)
            } else {
                format!(
                    "端口 {} 占用情况：\n{}",
                    port,
                    output_util::truncate_output_lines(
                        stdout.trim_end(),
                        max_output_len,
                        MAX_OUTPUT_LINES
                    )
                )
            }
        }
        Err(e) => {
            let reason = output_util::append_notfound_install_hint(format!("ss: {}", e), &e, "ss");
            try_lsof_fallback(port, max_output_len, &reason)
        }
    }
}

fn try_lsof_fallback(port: u16, max_output_len: usize, ss_err: &str) -> String {
    let mut cmd = Command::new("lsof");
    cmd.arg("-i").arg(format!(":{}", port)).arg("-P").arg("-n");
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                format!("端口 {} 未被占用（ss: {}; lsof 回退）", port, ss_err)
            } else {
                format!(
                    "端口 {} 占用情况（lsof 回退）：\n{}",
                    port,
                    output_util::truncate_output_lines(
                        stdout.trim_end(),
                        max_output_len,
                        MAX_OUTPUT_LINES
                    )
                )
            }
        }
        Err(e) => output_util::append_notfound_install_hint(
            format!("端口 {} 查询失败（ss: {}; lsof: {}）", port, ss_err, e),
            &e,
            "lsof",
        ),
    }
}

pub fn process_list(args_json: &str, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let filter = v
        .get("filter")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let user_only = v.get("user_only").and_then(|x| x.as_bool()).unwrap_or(true);
    let max_count = v.get("max_count").and_then(|x| x.as_u64()).unwrap_or(100) as usize;

    let mut cmd = Command::new("ps");
    if user_only {
        cmd.arg("ux");
    } else {
        cmd.arg("aux");
    }
    cmd.arg("--no-headers");

    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = if let Some(f) = filter {
                let fl = f.to_lowercase();
                stdout
                    .lines()
                    .filter(|line| line.to_lowercase().contains(&fl))
                    .take(max_count)
                    .collect()
            } else {
                stdout.lines().take(max_count).collect()
            };
            if lines.is_empty() {
                if let Some(f) = filter {
                    format!("未找到匹配 \"{}\" 的进程", f)
                } else {
                    "未找到进程".to_string()
                }
            } else {
                let header =
                    "USER       PID %CPU %MEM    VSZ   RSS TTY      STAT START   TIME COMMAND";
                let body = format!("{}\n{}", header, lines.join("\n"));
                format!(
                    "进程列表（共 {} 条）：\n{}",
                    lines.len(),
                    output_util::truncate_output_lines(&body, max_output_len, MAX_OUTPUT_LINES)
                )
            }
        }
        Err(e) => {
            output_util::append_notfound_install_hint(format!("ps: 无法执行（{}）", e), &e, "ps")
        }
    }
}

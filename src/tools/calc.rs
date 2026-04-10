//! 数学计算工具（bc -l）

use std::io::Write;
use std::process::{Command, Stdio};

use super::output_util;

/// 使用 bc -l 执行数学表达式，通过 stdin 传参，不经过 shell
pub fn run(expr: &str) -> String {
    let expr = expr.trim();
    if expr.is_empty() {
        return "错误：空表达式".to_string();
    }
    let bc_expr = expr
        .replace("math::sqrt(", "sqrt(")
        .replace("math::sin(", "s(")
        .replace("math::cos(", "c(")
        .replace("math::tan(", "t(")
        .replace("math::atan(", "a(")
        .replace("math::ln(", "l(")
        .replace("math::exp(", "e(")
        .replace("math::log10(", "log10(")
        .replace("math::log2(", "log2(");
    if !bc_expr
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "+-*/.^()% ".contains(c))
    {
        return "错误：表达式中含有不允许的字符".to_string();
    }
    let script = format!(
        "scale=15\npi=4*a(1)\ne=e(1)\ndefine t(x){{return s(x)/c(x)}}\ndefine log10(x){{return l(x)/l(10)}}\ndefine log2(x){{return l(x)/l(2)}}\n{}\n",
        bc_expr
    );
    let mut child = match Command::new("bc")
        .arg("-l")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let base = format!("无法执行 bc：{}（请确认已安装 bc）", e);
            return output_util::append_notfound_install_hint(base, &e, "bc");
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(script.as_bytes());
    }
    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return format!("bc 执行失败：{}", e),
    };
    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let detail = if err.trim().is_empty() {
            out.trim()
        } else {
            err.trim()
        };
        return format!("bc 错误：{}", detail);
    }
    let result = out.lines().last().unwrap_or("").trim();
    if result.is_empty() {
        "计算结果：(无输出)".to_string()
    } else {
        format!("计算结果：{}", result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_empty_expression() {
        assert_eq!(run(""), "错误：空表达式");
        assert_eq!(run("   "), "错误：空表达式");
    }

    #[test]
    fn test_run_simple_arithmetic() {
        let out = run("1+1");
        assert!(out.contains("2"), "1+1 应为 2，得到: {}", out);
    }

    #[test]
    fn test_run_disallowed_chars() {
        let out = run("1+1; ls");
        assert!(
            out.contains("不允许的字符"),
            "应拒绝含 ; 的表达式，得到: {}",
            out
        );
    }
}

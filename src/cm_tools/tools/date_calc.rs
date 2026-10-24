//! 日期计算工具（纯内存，基于 chrono）

use chrono::{Datelike, Duration, Local, NaiveDate};

use crate::cm_tools::tool_result::ToolError;

/// 显式失败版本（成功正文与迁移前的 String 面逐字节一致）。
///
/// 本族无内部 String 消费者，故不保留 `run` 门面；测试用本地 `run_str` 还原旧形态。
#[allow(clippy::result_large_err)]
pub fn run_try(args_json: &str) -> Result<String, ToolError> {
    let args: super::tool_param_types::DateCalcArgs =
        super::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    match args
        .mode
        .unwrap_or(super::tool_param_types::DateCalcMode::Offset)
    {
        super::tool_param_types::DateCalcMode::Diff => run_diff(&args),
        super::tool_param_types::DateCalcMode::Offset => run_offset(&args),
    }
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

#[allow(clippy::result_large_err)]
fn run_diff(args: &super::tool_param_types::DateCalcArgs) -> Result<String, ToolError> {
    let from_str = match args.from.as_deref() {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => {
            return Err(ToolError::invalid_args(
                "错误：diff 模式需要 from 参数（YYYY-MM-DD）".to_string(),
            ));
        }
    };
    let to_str = match args.to.as_deref() {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => {
            return Err(ToolError::invalid_args(
                "错误：diff 模式需要 to 参数（YYYY-MM-DD）".to_string(),
            ));
        }
    };
    let from = match parse_date(from_str) {
        Some(d) => d,
        None => {
            return Err(ToolError::invalid_args(format!(
                "错误：无法解析 from 日期 {:?}（格式 YYYY-MM-DD）",
                from_str
            )));
        }
    };
    let to = match parse_date(to_str) {
        Some(d) => d,
        None => {
            return Err(ToolError::invalid_args(format!(
                "错误：无法解析 to 日期 {:?}（格式 YYYY-MM-DD）",
                to_str
            )));
        }
    };
    let diff = to.signed_duration_since(from);
    let days = diff.num_days();
    let weeks = days / 7;
    let rem_days = days % 7;
    Ok(format!(
        "从 {} 到 {}：\n- 共 {} 天\n- 约 {} 周 {} 天",
        from,
        to,
        days,
        weeks,
        rem_days.abs()
    ))
}

#[allow(clippy::result_large_err)]
fn run_offset(args: &super::tool_param_types::DateCalcArgs) -> Result<String, ToolError> {
    let base_str = args.base.as_deref().unwrap_or("");
    let base = if base_str.trim().is_empty() {
        Local::now().date_naive()
    } else {
        match parse_date(base_str) {
            Some(d) => d,
            None => {
                return Err(ToolError::invalid_args(format!(
                    "错误：无法解析 base 日期 {:?}（格式 YYYY-MM-DD）",
                    base_str
                )));
            }
        }
    };
    let offset_str = match args.offset.as_deref() {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => {
            return Err(ToolError::invalid_args(
                "错误：offset 模式需要 offset 参数（如 +30d, -2w, +1m）".to_string(),
            ));
        }
    };
    let (sign, rest) = if let Some(r) = offset_str.strip_prefix('+') {
        (1i64, r)
    } else if let Some(r) = offset_str.strip_prefix('-') {
        (-1i64, r)
    } else {
        (1i64, offset_str)
    };
    let Some((unit_start, _)) = rest.char_indices().last() else {
        return Err(ToolError::invalid_args(format!(
            "错误：无法解析偏移量 {:?}",
            offset_str
        )));
    };
    let (num_str, unit) = rest.split_at(unit_start);
    let num: i64 = match num_str.parse::<i64>() {
        Ok(n) => n * sign,
        Err(_) => {
            return Err(ToolError::invalid_args(format!(
                "错误：无法解析偏移量数值 {:?}",
                offset_str
            )));
        }
    };
    let result = match unit {
        "d" => base + Duration::days(num),
        "w" => base + Duration::weeks(num),
        "m" => {
            let month = base.month0() as i32 + num as i32;
            let year = base.year() + month.div_euclid(12);
            let m = (month.rem_euclid(12) + 1) as u32;
            let d = base.day().min(days_in_month(year, m));
            NaiveDate::from_ymd_opt(year, m, d).unwrap_or(base)
        }
        _ => {
            return Err(ToolError::invalid_args(format!(
                "错误：未知偏移单位 {:?}（支持 d/w/m）",
                unit
            )));
        }
    };
    Ok(format!(
        "基准：{}\n偏移：{}\n结果：{}\n星期：{}",
        base,
        offset_str,
        result,
        result.format("%A")
    ))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    NaiveDate::from_ymd_opt(
        if month == 12 { year + 1 } else { year },
        if month == 12 { 1 } else { month + 1 },
        1,
    )
    .map(|d| d.pred_opt().map(|p| p.day()).unwrap_or(28))
    .unwrap_or(28)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 迁移前 String 面的等价物（`ToolError` → `message`），供断言旧文案用。
    fn run_str(args_json: &str) -> String {
        run_try(args_json).unwrap_or_else(|e| e.message)
    }

    #[test]
    fn diff_basic() {
        let out = run_str(r#"{"mode":"diff","from":"2024-01-01","to":"2024-01-31"}"#);
        assert!(out.contains("30 天"), "out={}", out);
    }

    #[test]
    fn offset_days() {
        let out = run_str(r#"{"mode":"offset","base":"2024-03-01","offset":"+10d"}"#);
        assert!(out.contains("2024-03-11"), "out={}", out);
    }

    #[test]
    fn offset_weeks() {
        let out = run_str(r#"{"mode":"offset","base":"2024-01-01","offset":"+2w"}"#);
        assert!(out.contains("2024-01-15"), "out={}", out);
    }

    #[test]
    fn offset_negative() {
        let out = run_str(r#"{"mode":"offset","base":"2024-03-15","offset":"-15d"}"#);
        assert!(out.contains("2024-02-29"), "out={}", out);
    }

    #[test]
    fn missing_offset() {
        let out = run_str(r#"{"mode":"offset","base":"2024-01-01"}"#);
        assert!(out.contains("错误"));
    }
}

//! 获取当前时间工具（含当月日历）

use chrono::{Datelike, NaiveDate};

/// 返回当前本地日期时间与当月日历
pub fn run() -> String {
    let now = chrono::Local::now();
    let time_str = format!("当前时间：{}", now.format("%Y-%m-%d %H:%M:%S"));
    let cal = format_month_calendar(now.year(), now.month());
    format!("{}\n\n{}", time_str, cal)
}

/// 格式化当月日历（中文星期头：日 一 二 … 六）
pub(crate) fn format_month_calendar(year: i32, month: u32) -> String {
    let first = match NaiveDate::from_ymd_opt(year, month, 1) {
        Some(d) => d,
        None => return String::new(),
    };
    let last_day = (1..=31)
        .rev()
        .find_map(|d| NaiveDate::from_ymd_opt(year, month, d))
        .map(|d| d.day())
        .unwrap_or(28);
    let wd_first = first.weekday().num_days_from_sunday() as usize; // 0=日, 1=一, ...

    let title = format!("{}年{}月", year, month);
    let title_pad = (7 * 3 - title.chars().count().max(1)) / 2;
    let mut out = format!("{}{}\n", " ".repeat(title_pad), title);
    out.push_str(" 日 一 二 三 四 五 六\n");

    let mut line = " ".repeat(wd_first * 3);
    for day in 1..=last_day {
        if day > 1 && (wd_first + (day as usize) - 1) % 7 == 0 {
            out.push_str(line.trim_end());
            out.push('\n');
            line = String::new();
        }
        line.push_str(&format!("{:>3}", day));
    }
    if !line.trim().is_empty() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_contains_time_and_calendar() {
        let out = run();
        assert!(out.contains("当前时间"), "应包含「当前时间」，得到: {}", out);
        assert!(out.contains("月"), "应包含当月日历，得到: {}", out);
    }

    #[test]
    fn test_format_month_calendar_has_days() {
        let out = format_month_calendar(2025, 3);
        assert!(!out.is_empty());
        assert!(out.contains("2025"));
        assert!(out.contains("3"));
        assert!(out.contains("日 一 二"));
    }
}

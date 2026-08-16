//! 从 `CM_*` 读取并写入配置字段，避免覆盖函数内堆叠 `if let`（降低 lizard CCN）。

use std::str::FromStr;

use super::source::parse_bool_like;

fn env_ok(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn csv_nonempty_parts(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// 解析 trim 后的整数/浮点；无效或缺省则不动 `dest`。
pub(crate) fn apply_parse<T: FromStr>(dest: &mut Option<T>, key: &str) {
    let Some(v) = env_ok(key) else {
        return;
    };
    let Ok(n) = v.trim().parse::<T>() else {
        return;
    };
    *dest = Some(n);
}

pub(crate) fn apply_bool(dest: &mut Option<bool>, key: &str) {
    let Some(v) = env_ok(key) else {
        return;
    };
    let Some(val) = parse_bool_like(&v) else {
        return;
    };
    *dest = Some(val);
}

/// 非空 trim 后覆盖 `String`。
pub(crate) fn apply_nonempty_string(dest: &mut String, key: &str) {
    let Some(v) = env_ok(key) else {
        return;
    };
    let s = v.trim().to_string();
    if s.is_empty() {
        return;
    }
    *dest = s;
}

/// 非空 trim 后覆盖 `String`，并清空关联的文件路径字段。
/// 调用方若接着应用 `CM_*_FILE`，则文件路径仍可覆盖（与历史语义一致）。
pub(crate) fn apply_nonempty_string_clearing_opt(
    dest: &mut String,
    file: &mut Option<String>,
    key: &str,
) {
    let Some(v) = env_ok(key) else {
        return;
    };
    let s = v.trim().to_string();
    if s.is_empty() {
        return;
    }
    *dest = s;
    *file = None;
}

/// 非空 trim 后写入 `Option<String>`。
pub(crate) fn apply_nonempty_opt(dest: &mut Option<String>, key: &str) {
    let Some(v) = env_ok(key) else {
        return;
    };
    let s = v.trim().to_string();
    if s.is_empty() {
        return;
    }
    *dest = Some(s);
}

/// 非空 trim 后写入内联文本并清空对应 `*_file`。
pub(crate) fn apply_nonempty_opt_clearing_file(
    dest: &mut Option<String>,
    file: &mut Option<String>,
    key: &str,
) {
    let Some(v) = env_ok(key) else {
        return;
    };
    let s = v.trim().to_string();
    if s.is_empty() {
        return;
    }
    *dest = Some(s);
    *file = None;
}

/// trim 后写入（允许空串，例如显式清空目录或 token）。
pub(crate) fn apply_trimmed_opt(dest: &mut Option<String>, key: &str) {
    let Some(v) = env_ok(key) else {
        return;
    };
    *dest = Some(v.trim().to_string());
}

/// 原样写入（不 trim）。
pub(crate) fn apply_raw_opt(dest: &mut Option<String>, key: &str) {
    let Some(v) = env_ok(key) else {
        return;
    };
    *dest = Some(v);
}

/// 逗号分隔列表；全空则不覆盖。
pub(crate) fn apply_csv_nonempty(dest: &mut Option<Vec<String>>, key: &str) {
    let Some(v) = env_ok(key) else {
        return;
    };
    let list = csv_nonempty_parts(&v);
    if list.is_empty() {
        return;
    }
    *dest = Some(list);
}

/// 逗号分隔列表；变量存在则覆盖（空列表表示显式清空，如 CORS）。
pub(crate) fn apply_csv_allow_empty(dest: &mut Option<Vec<String>>, key: &str) {
    let Some(v) = env_ok(key) else {
        return;
    };
    *dest = Some(csv_nonempty_parts(&v));
}

pub(crate) fn env_flag_true(key: &str) -> bool {
    let Some(v) = env_ok(key) else {
        return false;
    };
    parse_bool_like(&v) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::csv_nonempty_parts;

    #[test]
    fn csv_nonempty_parts_trims_and_drops_blanks() {
        assert_eq!(
            csv_nonempty_parts(" a, ,b,c "),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert!(csv_nonempty_parts(" , , ").is_empty());
    }
}

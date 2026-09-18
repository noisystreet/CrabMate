//! 从 `CM_*` 读取并写入配置字段，避免覆盖函数内堆叠 `if let`（降低 lizard CCN）。

use std::str::FromStr;

use super::source::parse_bool_like;

/// 测试期在当前线程内替换 [`env_ok`] 的环境读取视图（本模块 `CM_*` 覆盖均经该函数）。
///
/// 环境变量是**进程级**的：并行用例一旦 `set_var`，该「已设置」窗口对其它线程可见，会让未持锁的
/// `load_config` 用例读到脏值（历史 flake：`CM_PLANNER_EXECUTOR_MODE=logical_dual_agent` 使
/// 别处的 `load_config` 直接报错）。把覆盖限制在**当前线程**后，用例不再写进程环境，也就不再需要
/// 各写各的互斥锁。
///
/// 注意：`scope` 只能「设值」，不能表达「取消该键」；对 `apply_nonempty_opt` / `apply_parse` /
/// `apply_bool` 而言空串等价「未设置」，但 [`apply_csv_allow_empty`] 的空串语义是**显式清空**。
#[cfg(test)]
pub(crate) mod scoped_env {
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        /// `None`：未启用替换，读真实环境；`Some(map)`：只看该表，缺失键视为「未设置」。
        static OVERRIDES: RefCell<Option<HashMap<String, String>>> = const { RefCell::new(None) };
    }

    /// 返回 `None` 表示未启用替换；`Some(None)` 表示已启用但该键不存在。
    pub(crate) fn lookup(key: &str) -> Option<Option<String>> {
        OVERRIDES.with(|o| o.borrow().as_ref().map(|m| m.get(key).cloned()))
    }

    /// 作用域内以 `vars` 覆盖当前线程的环境视图；退出（含 panic）后恢复上一层，支持嵌套。
    pub(crate) fn scope(vars: &[(&str, &str)]) -> Scope {
        let prev = OVERRIDES.with(|o| o.borrow_mut().take());
        let mut map = prev.clone().unwrap_or_default();
        for (k, v) in vars {
            map.insert((*k).to_string(), (*v).to_string());
        }
        OVERRIDES.with(|o| *o.borrow_mut() = Some(map));
        Scope(prev)
    }

    #[must_use]
    pub(crate) struct Scope(Option<HashMap<String, String>>);

    impl Drop for Scope {
        fn drop(&mut self) {
            OVERRIDES.with(|o| *o.borrow_mut() = self.0.take());
        }
    }
}

fn env_ok(key: &str) -> Option<String> {
    #[cfg(test)]
    {
        if let Some(scoped) = scoped_env::lookup(key) {
            return scoped;
        }
    }
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

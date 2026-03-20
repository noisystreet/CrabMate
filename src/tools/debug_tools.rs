//! 调试辅助工具：Rust panic/backtrace 解析

use std::collections::BTreeMap;

pub fn rust_backtrace_analyze(args_json: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let text = match v.get("backtrace").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return "错误：缺少 backtrace 参数".to_string(),
    };
    let crate_hint = v
        .get("crate_hint")
        .and_then(|x| x.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let mut frame_hits: Vec<String> = Vec::new();
    let mut module_count: BTreeMap<String, usize> = BTreeMap::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if !l.contains("::") {
            continue;
        }
        if let Some(hint) = crate_hint {
            if !l.contains(hint) {
                continue;
            }
        } else if is_noise_frame(l) {
            continue;
        }
        frame_hits.push(l.to_string());
        let module = l
            .split("::")
            .take(2)
            .collect::<Vec<_>>()
            .join("::")
            .trim()
            .to_string();
        if !module.is_empty() {
            *module_count.entry(module).or_insert(0) += 1;
        }
    }

    if frame_hits.is_empty() {
        return "未识别到可分析的业务调用栈帧。可尝试传入 crate_hint（如你的 crate 名）。".to_string();
    }

    let first = frame_hits.first().cloned().unwrap_or_default();
    let mut top_modules = module_count.into_iter().collect::<Vec<_>>();
    top_modules.sort_by(|a, b| b.1.cmp(&a.1));

    let mut out = String::new();
    out.push_str("backtrace 分析结果:\n");
    out.push_str(&format!("- 首个可疑业务帧: {}\n", first));
    out.push_str("- 主要模块命中:\n");
    for (name, count) in top_modules.into_iter().take(5) {
        out.push_str(&format!("  - {}: {} 次\n", name, count));
    }
    out.push_str("- 建议: 优先从首个可疑业务帧对应函数开始排查参数、unwrap、索引越界和并发共享状态。");
    out
}

fn is_noise_frame(line: &str) -> bool {
    let noise = [
        "std::",
        "core::",
        "tokio::",
        "alloc::",
        "panic_unwind",
        "backtrace",
    ];
    noise.iter().any(|n| line.contains(n))
}


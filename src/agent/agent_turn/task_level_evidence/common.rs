//! 任务级验收：共享小工具与期望输出提示（从单文件拆出，避免 lizard 对 `r#""#` 的误解析）。

/// 用户任务是否像「写 C++ 程序并编译运行」类请求（用于任务级验收门控）。
pub(crate) fn is_program_build_run_request(task: &str) -> bool {
    let t = task.to_lowercase();
    let asks_write = t.contains("编写") || t.contains("实现") || t.contains("write");
    let asks_program = t.contains("程序") || t.contains("c++") || t.contains("cpp");
    let asks_run = t.contains("执行")
        || t.contains("运行")
        || t.contains("编译")
        || t.contains("build")
        || t.contains("run");
    asks_write && asks_program && asks_run
}

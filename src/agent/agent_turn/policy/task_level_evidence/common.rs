//! 任务级验收：共享小工具与期望输出提示（从单文件拆出，避免 lizard 对 `r#""#` 的误解析）。

fn task_contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

/// 用户任务是否像「写 C++ 程序并编译运行」类请求（用于任务级验收门控）。
pub fn is_program_build_run_request(task: &str) -> bool {
    let t = task.to_lowercase();
    const WRITE: &[&str] = &["编写", "实现", "write"];
    const PROGRAM: &[&str] = &["程序", "c++", "cpp"];
    const RUN: &[&str] = &["执行", "运行", "编译", "build", "run"];
    task_contains_any(&t, WRITE) && task_contains_any(&t, PROGRAM) && task_contains_any(&t, RUN)
}

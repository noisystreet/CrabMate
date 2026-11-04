//! `--benchmark` / `--batch` 批量评测入口（S5-a 第三步：从 [`crate::cli_run`] 拆出，纯搬移）。
//!
//! 该函数**没有 `cfg` 门**（两套构建都有），被 `cli_run_session` 无条件调用。
//! 早期设计稿曾误判它只在非 web 构建存在——那是 `#[cfg]` 只作用于紧跟项的经典误读，已更正。

use crate::config;
use crate::runtime;

/// `--benchmark` / `--batch`：跑批量评测后返回 `true`（调用方应结束进程）。
pub(super) async fn run_benchmark_batch_if_requested(
    cfg_holder: &config::SharedAgentConfig,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    bench_args: &config::cli::definitions::BenchmarkCliArgs,
) -> Result<bool, Box<dyn std::error::Error>> {
    if bench_args.benchmark.is_none() && bench_args.batch.is_none() {
        return Ok(false);
    }
    let bench_kind_str = bench_args.benchmark.as_deref().unwrap_or("generic");
    let bench_kind = runtime::benchmark::types::BenchmarkKind::parse(bench_kind_str)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let batch_input = bench_args.batch.as_deref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "使用 --benchmark 时必须同时指定 --batch <INPUT.jsonl>",
        )
    })?;
    let batch_output = bench_args
        .batch_output
        .as_deref()
        .unwrap_or("benchmark_results.jsonl");

    let system_prompt_override = match bench_args.system_prompt_file.as_deref() {
        Some(path) => {
            let content = std::fs::read_to_string(path).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("无法读取 bench-system-prompt 文件 {path}: {e}"),
                )
            })?;
            Some(content)
        }
        None => None,
    };

    let batch_cfg = runtime::benchmark::types::BatchRunConfig {
        benchmark: bench_kind,
        input_path: batch_input.to_string(),
        output_path: batch_output.to_string(),
        task_timeout_secs: bench_args.task_timeout,
        max_tool_rounds: bench_args.max_tool_rounds,
        samples: bench_args.samples,
        resume_from_existing: bench_args.resume,
        system_prompt_override,
    };

    runtime::benchmark::runner::run_batch(cfg_holder, client, api_key, tools, &batch_cfg).await?;
    Ok(true)
}

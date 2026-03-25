//! Batch runner：批量执行 benchmark 任务的主循环。
//!
//! 读取 JSONL 输入 → per-task 隔离执行 → 逐条写入输出 JSONL → 最终输出汇总。

use super::adapter::{BenchmarkAdapter, create_adapter};
use super::metrics::{BatchSummary, TaskMetrics};
use super::types::{BatchRunConfig, BenchmarkResult, BenchmarkTask, TaskStatus};
use crate::config::AgentConfig;
use crate::types::Tool;
use log::{error, info, warn};
use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// 批量运行入口。
pub async fn run_batch(
    cfg: &Arc<AgentConfig>,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[Tool],
    batch_cfg: &BatchRunConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let adapter = create_adapter(batch_cfg.benchmark);
    info!(
        target: "crabmate::benchmark",
        "批量测评开始 benchmark={} input={} output={}",
        batch_cfg.benchmark.as_str(),
        batch_cfg.input_path,
        batch_cfg.output_path,
    );

    let tasks = load_tasks(&batch_cfg.input_path)?;
    if tasks.is_empty() {
        eprintln!("输入文件为空或无有效任务");
        return Ok(());
    }
    eprintln!(
        "[benchmark] 已加载 {} 条任务 ({})",
        tasks.len(),
        batch_cfg.benchmark.as_str()
    );

    let existing_ids = if batch_cfg.resume_from_existing {
        load_existing_ids(&batch_cfg.output_path)
    } else {
        HashSet::new()
    };

    let mut results: Vec<BenchmarkResult> = Vec::new();
    let base_work_dir = std::path::Path::new(&cfg.run_command_working_dir)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(&cfg.run_command_working_dir));

    let mut out_file = open_output_file(&batch_cfg.output_path, batch_cfg.resume_from_existing)?;

    for (idx, task) in tasks.iter().enumerate() {
        if existing_ids.contains(&task.instance_id) {
            eprintln!(
                "[benchmark] [{}/{}] 跳过已有结果: {}",
                idx + 1,
                tasks.len(),
                task.instance_id
            );
            continue;
        }

        eprintln!(
            "[benchmark] [{}/{}] 开始: {}",
            idx + 1,
            tasks.len(),
            task.instance_id
        );

        let result = run_single_task(
            cfg,
            client,
            api_key,
            tools,
            adapter.as_ref(),
            task,
            &base_work_dir,
            batch_cfg,
        )
        .await;

        eprintln!(
            "[benchmark] [{}/{}] 完成: {} status={:?} time={:.1}s",
            idx + 1,
            tasks.len(),
            task.instance_id,
            result.status,
            result.metrics.wall_time_secs,
        );

        write_result_line(&mut out_file, &result)?;
        results.push(result);
    }

    let summary = BatchSummary::from_results(&results);
    let summary_path = summary_path_from_output(&batch_cfg.output_path);
    write_summary(&summary_path, &summary)?;

    eprintln!("\n[benchmark] 批量测评完成");
    eprintln!(
        "  总任务: {}  成功: {}  超时: {}  错误: {}  达到轮次上限: {}",
        summary.total_tasks,
        summary.success_count,
        summary.timeout_count,
        summary.error_count,
        summary.max_rounds_count,
    );
    eprintln!(
        "  平均耗时: {:.1}s  总工具调用: {}",
        summary.avg_wall_time_secs, summary.total_tool_calls,
    );
    eprintln!("  结果: {}", batch_cfg.output_path);
    eprintln!("  汇总: {}", summary_path);

    Ok(())
}

/// 执行单条任务。
#[allow(clippy::too_many_arguments)]
async fn run_single_task(
    cfg: &Arc<AgentConfig>,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[Tool],
    adapter: &dyn BenchmarkAdapter,
    task: &BenchmarkTask,
    base_work_dir: &Path,
    batch_cfg: &BatchRunConfig,
) -> BenchmarkResult {
    if let Err(e) = adapter.validate_task(task) {
        return adapter.extract_result(
            task,
            None,
            base_work_dir,
            TaskStatus::Error,
            TaskMetrics::default(),
            &cfg.model,
            Some(format!("输入校验失败: {e}")),
        );
    }

    let work_dir = match adapter.setup_workspace(task, base_work_dir) {
        Ok(d) => d,
        Err(e) => {
            error!(
                target: "crabmate::benchmark",
                "工作区初始化失败 {}: {e}",
                task.instance_id,
            );
            return adapter.extract_result(
                task,
                None,
                base_work_dir,
                TaskStatus::Error,
                TaskMetrics::default(),
                &cfg.model,
                Some(format!("工作区初始化失败: {e}")),
            );
        }
    };

    let task_cfg = build_task_config(cfg, adapter, batch_cfg);
    let user_prompt = adapter.build_user_prompt(task);
    let mut messages = crate::types::messages_chat_seed(&task_cfg.system_prompt, &user_prompt);

    let start = Instant::now();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let timeout_secs = batch_cfg.task_timeout_secs;
    let run_fut = crate::run_agent_turn(
        client,
        api_key,
        &task_cfg,
        tools,
        &mut messages,
        None,
        &work_dir,
        true,
        false,
        true, // no_stream：batch 模式不需要流式输出
        Some(cancel.clone()),
        None,
        None,
        false,
    );

    let (status, agent_error) = if timeout_secs > 0 {
        match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), run_fut).await {
            Ok(Ok(())) => (TaskStatus::Success, None),
            Ok(Err(e)) => {
                let msg = e.to_string();
                warn!(
                    target: "crabmate::benchmark",
                    "任务执行出错 {}: {msg}",
                    task.instance_id,
                );
                (TaskStatus::Error, Some(msg))
            }
            Err(_elapsed) => {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                warn!(
                    target: "crabmate::benchmark",
                    "任务超时 {}: {timeout_secs}s",
                    task.instance_id,
                );
                (TaskStatus::Timeout, Some(format!("超时 ({timeout_secs}s)")))
            }
        }
    } else {
        match run_fut.await {
            Ok(()) => (TaskStatus::Success, None),
            Err(e) => {
                let msg = e.to_string();
                (TaskStatus::Error, Some(msg))
            }
        }
    };

    let wall_time = start.elapsed().as_secs_f64();

    let raw_reply = messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .and_then(|m| m.content.clone());

    let tool_calls_count = messages
        .iter()
        .filter(|m| m.role == "assistant" && m.tool_calls.is_some())
        .map(|m| m.tool_calls.as_ref().map_or(0, |tc| tc.len()))
        .sum();

    let agent_rounds = messages.iter().filter(|m| m.role == "assistant").count();

    let metrics = TaskMetrics {
        wall_time_secs: wall_time,
        tool_calls_count,
        agent_rounds,
    };

    adapter.extract_result(
        task,
        raw_reply.as_deref(),
        &work_dir,
        status,
        metrics,
        &cfg.model,
        agent_error,
    )
}

/// 构建任务专用的 AgentConfig（可能覆盖 system_prompt、max_message_history 等）。
fn build_task_config(
    base: &Arc<AgentConfig>,
    adapter: &dyn BenchmarkAdapter,
    batch_cfg: &BatchRunConfig,
) -> Arc<AgentConfig> {
    let mut cfg = (**base).clone();

    if let Some(ref override_prompt) = batch_cfg.system_prompt_override {
        cfg.system_prompt = override_prompt.clone();
    }

    if let Some(suffix) = adapter.system_prompt_suffix() {
        if !cfg.system_prompt.is_empty() {
            cfg.system_prompt.push('\n');
        }
        cfg.system_prompt.push_str(&suffix);
    }

    // max_tool_rounds > 0 时，用 max_message_history 近似限制轮次
    // （每轮 assistant + tool 约 2 条消息，加上 system + user 起始 2 条）
    if batch_cfg.max_tool_rounds > 0 {
        let estimated_max = 2 + batch_cfg.max_tool_rounds * 3;
        if estimated_max < cfg.max_message_history || cfg.max_message_history == 0 {
            cfg.max_message_history = estimated_max;
        }
    }

    Arc::new(cfg)
}

// ---------------------------------------------------------------------------
// I/O 辅助
// ---------------------------------------------------------------------------

fn load_tasks(path: &str) -> Result<Vec<BenchmarkTask>, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path).map_err(|e| format!("无法打开输入文件 {path}: {e}"))?;
    let reader = std::io::BufReader::new(file);
    let mut tasks = Vec::new();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| format!("读取第 {} 行失败: {e}", line_num + 1))?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match serde_json::from_str::<BenchmarkTask>(trimmed) {
            Ok(task) => tasks.push(task),
            Err(e) => {
                warn!(
                    target: "crabmate::benchmark",
                    "跳过第 {} 行（JSON 解析失败）: {e}",
                    line_num + 1,
                );
            }
        }
    }
    Ok(tasks)
}

fn load_existing_ids(path: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(file) = std::fs::File::open(path) else {
        return ids;
    };
    let reader = std::io::BufReader::new(file);
    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if let Ok(r) = serde_json::from_str::<BenchmarkResult>(trimmed) {
            ids.insert(r.instance_id);
        }
    }
    ids
}

fn open_output_file(path: &str, append: bool) -> Result<std::fs::File, Box<dyn std::error::Error>> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(append)
        .truncate(!append)
        .write(true)
        .open(path)
        .map_err(|e| format!("无法打开输出文件 {path}: {e}"))?;
    Ok(file)
}

fn write_result_line(
    file: &mut std::fs::File,
    result: &BenchmarkResult,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(result)?;
    writeln!(file, "{json}")?;
    file.flush()?;
    Ok(())
}

fn summary_path_from_output(output_path: &str) -> String {
    let p = Path::new(output_path);
    let stem = p.file_stem().unwrap_or_default().to_string_lossy();
    let parent = p.parent().unwrap_or(Path::new("."));
    parent
        .join(format!("{stem}_summary.json"))
        .to_string_lossy()
        .to_string()
}

fn write_summary(path: &str, summary: &BatchSummary) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(summary)?;
    std::fs::write(path, json)?;
    eprintln!("[benchmark] 汇总已写入 {path}");
    Ok(())
}

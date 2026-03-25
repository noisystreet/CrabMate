//! Benchmark 适配器：为每种 benchmark 定义输入解析、工作区 setup、system prompt 定制和结果提取。

use super::artifact;
use super::types::{BenchmarkKind, BenchmarkResult, BenchmarkTask, TaskStatus};
use std::path::{Path, PathBuf};

/// 适配器统一接口：每种 benchmark 实现此 trait。
#[allow(clippy::too_many_arguments)]
pub trait BenchmarkAdapter: Send + Sync {
    fn kind(&self) -> BenchmarkKind;

    /// 校验任务输入中必填字段是否齐全。
    fn validate_task(&self, task: &BenchmarkTask) -> Result<(), String>;

    /// 为本次任务生成发给 agent 的 user message 文本。
    fn build_user_prompt(&self, task: &BenchmarkTask) -> String;

    /// 追加到 system prompt 的 benchmark 专属指令（可选，返回 None 则用默认 system prompt）。
    fn system_prompt_suffix(&self) -> Option<String> {
        None
    }

    /// 任务开始前的工作区初始化（如 clone repo、checkout commit）。
    /// 返回实际使用的工作目录（可能是新建的临时目录）。
    fn setup_workspace(
        &self,
        task: &BenchmarkTask,
        base_work_dir: &Path,
    ) -> Result<PathBuf, String>;

    /// agent 执行完成后，从 messages / 工作区中提取结果产物并组装为 `BenchmarkResult`。
    fn extract_result(
        &self,
        task: &BenchmarkTask,
        raw_reply: Option<&str>,
        work_dir: &Path,
        status: TaskStatus,
        metrics: super::metrics::TaskMetrics,
        model_name: &str,
        error: Option<String>,
    ) -> BenchmarkResult;
}

// ---------------------------------------------------------------------------
// SWE-bench Adapter
// ---------------------------------------------------------------------------

pub struct SweBenchAdapter;

impl BenchmarkAdapter for SweBenchAdapter {
    fn kind(&self) -> BenchmarkKind {
        BenchmarkKind::SweBench
    }

    fn validate_task(&self, task: &BenchmarkTask) -> Result<(), String> {
        if task.repo.is_none() {
            return Err(format!("{}: 缺少 repo 字段", task.instance_id));
        }
        if task.base_commit.is_none() {
            return Err(format!("{}: 缺少 base_commit 字段", task.instance_id));
        }
        if task.problem_statement.is_none() && task.prompt.is_empty() {
            return Err(format!(
                "{}: 缺少 problem_statement 或 prompt 字段",
                task.instance_id
            ));
        }
        Ok(())
    }

    fn build_user_prompt(&self, task: &BenchmarkTask) -> String {
        let problem = task
            .problem_statement
            .as_deref()
            .unwrap_or(task.prompt.as_str());
        let hints = task.hints_text.as_deref().unwrap_or("");
        let mut prompt = format!(
            "请修复以下 GitHub Issue。仓库已在当前工作区中 checkout 到正确的基线提交。\n\n\
             ## Issue\n\n{problem}\n"
        );
        if !hints.is_empty() {
            prompt.push_str(&format!("\n## Hints\n\n{hints}\n"));
        }
        prompt.push_str(
            "\n请直接修改工作区中的代码文件来修复此 issue。\
             不要创建新的测试文件，只修改源代码。完成后无需 commit。",
        );
        prompt
    }

    fn system_prompt_suffix(&self) -> Option<String> {
        Some(
            "你是一个专业的软件工程 Agent。当前工作区是一个 Git 仓库，已 checkout 到指定的基线提交。\
             你的任务是阅读 Issue 描述，定位 bug 或需求，然后修改源代码来解决问题。\
             请确保修改尽可能最小化，只改动解决问题所必须的代码。不要提交 (git commit)，只修改文件即可。"
                .to_string(),
        )
    }

    fn setup_workspace(
        &self,
        task: &BenchmarkTask,
        base_work_dir: &Path,
    ) -> Result<PathBuf, String> {
        let repo = task.repo.as_deref().ok_or("缺少 repo")?;
        let commit = task.base_commit.as_deref().ok_or("缺少 base_commit")?;

        let safe_name = task.instance_id.replace(['/', '\\', ' '], "_");
        let task_dir = base_work_dir.join(&safe_name);

        if task_dir.exists() {
            // 已存在（可能是续跑），尝试 reset
            run_git_cmd(&task_dir, &["checkout", "--force", commit])?;
            run_git_cmd(&task_dir, &["clean", "-fdx"])?;
            return Ok(task_dir);
        }

        let repo_url = if repo.starts_with("http://") || repo.starts_with("https://") {
            repo.to_string()
        } else {
            format!("https://github.com/{repo}.git")
        };

        run_git_cmd_in(base_work_dir, &["clone", "--quiet", &repo_url, &safe_name])?;
        run_git_cmd(&task_dir, &["checkout", "--force", commit])?;

        Ok(task_dir)
    }

    fn extract_result(
        &self,
        task: &BenchmarkTask,
        raw_reply: Option<&str>,
        work_dir: &Path,
        status: TaskStatus,
        metrics: super::metrics::TaskMetrics,
        model_name: &str,
        error: Option<String>,
    ) -> BenchmarkResult {
        let patch = artifact::extract_git_patch(work_dir).ok();
        BenchmarkResult {
            instance_id: task.instance_id.clone(),
            benchmark: self.kind().as_str().to_string(),
            status,
            raw_reply: raw_reply.map(|s| s.to_string()),
            model_patch: patch,
            final_answer: None,
            completion: None,
            metrics,
            model_name_or_path: model_name.to_string(),
            error,
        }
    }
}

// ---------------------------------------------------------------------------
// GAIA Adapter
// ---------------------------------------------------------------------------

pub struct GaiaAdapter;

impl BenchmarkAdapter for GaiaAdapter {
    fn kind(&self) -> BenchmarkKind {
        BenchmarkKind::Gaia
    }

    fn validate_task(&self, task: &BenchmarkTask) -> Result<(), String> {
        if task.prompt.is_empty() {
            return Err(format!("{}: 缺少 prompt/question 字段", task.instance_id));
        }
        Ok(())
    }

    fn build_user_prompt(&self, task: &BenchmarkTask) -> String {
        let mut prompt = task.prompt.clone();
        if !task.file_attachments.is_empty() {
            prompt.push_str("\n\n附件文件路径：\n");
            for f in &task.file_attachments {
                prompt.push_str(&format!("- {f}\n"));
            }
        }
        prompt
    }

    fn system_prompt_suffix(&self) -> Option<String> {
        Some(
            "你是一个通用 AI 助手，具有使用工具（网络搜索、文件读取、计算等）的能力。\
             请仔细分析问题，按需使用工具收集信息，然后给出最终答案。\
             你的最终答案必须以以下格式给出：\n\
             FINAL ANSWER: <你的答案>\n\n\
             答案应尽可能简洁：\n\
             - 如果答案是数字，直接给出数字（不要逗号分隔符，除非题目要求）\n\
             - 如果答案是字符串，不要缩写\n\
             - 如果答案是列表，用逗号分隔"
                .to_string(),
        )
    }

    fn setup_workspace(
        &self,
        _task: &BenchmarkTask,
        base_work_dir: &Path,
    ) -> Result<PathBuf, String> {
        Ok(base_work_dir.to_path_buf())
    }

    fn extract_result(
        &self,
        task: &BenchmarkTask,
        raw_reply: Option<&str>,
        _work_dir: &Path,
        status: TaskStatus,
        metrics: super::metrics::TaskMetrics,
        model_name: &str,
        error: Option<String>,
    ) -> BenchmarkResult {
        let answer = raw_reply.and_then(artifact::extract_final_answer);
        BenchmarkResult {
            instance_id: task.instance_id.clone(),
            benchmark: self.kind().as_str().to_string(),
            status,
            raw_reply: raw_reply.map(|s| s.to_string()),
            model_patch: None,
            final_answer: answer,
            completion: None,
            metrics,
            model_name_or_path: model_name.to_string(),
            error,
        }
    }
}

// ---------------------------------------------------------------------------
// HumanEval Adapter
// ---------------------------------------------------------------------------

pub struct HumanEvalAdapter;

impl BenchmarkAdapter for HumanEvalAdapter {
    fn kind(&self) -> BenchmarkKind {
        BenchmarkKind::HumanEval
    }

    fn validate_task(&self, task: &BenchmarkTask) -> Result<(), String> {
        if task.prompt.is_empty() {
            return Err(format!("{}: 缺少 prompt 字段", task.instance_id));
        }
        Ok(())
    }

    fn build_user_prompt(&self, task: &BenchmarkTask) -> String {
        format!(
            "请补全以下 Python 函数。只输出函数体的代码，不要重复函数签名和 docstring。\n\n```python\n{}\n```",
            task.prompt
        )
    }

    fn system_prompt_suffix(&self) -> Option<String> {
        Some(
            "你是一个 Python 编程专家。请严格按要求补全代码。\
             只输出补全部分的代码，用 ```python 代码块包裹。\
             不要输出函数签名和文档字符串，不要输出测试代码，只输出函数体实现。"
                .to_string(),
        )
    }

    fn setup_workspace(
        &self,
        _task: &BenchmarkTask,
        base_work_dir: &Path,
    ) -> Result<PathBuf, String> {
        Ok(base_work_dir.to_path_buf())
    }

    fn extract_result(
        &self,
        task: &BenchmarkTask,
        raw_reply: Option<&str>,
        _work_dir: &Path,
        status: TaskStatus,
        metrics: super::metrics::TaskMetrics,
        model_name: &str,
        error: Option<String>,
    ) -> BenchmarkResult {
        let completion = raw_reply.map(artifact::extract_code_completion);
        BenchmarkResult {
            instance_id: task.instance_id.clone(),
            benchmark: self.kind().as_str().to_string(),
            status,
            raw_reply: raw_reply.map(|s| s.to_string()),
            model_patch: None,
            final_answer: None,
            completion,
            metrics,
            model_name_or_path: model_name.to_string(),
            error,
        }
    }
}

// ---------------------------------------------------------------------------
// Generic Adapter
// ---------------------------------------------------------------------------

pub struct GenericAdapter;

impl BenchmarkAdapter for GenericAdapter {
    fn kind(&self) -> BenchmarkKind {
        BenchmarkKind::Generic
    }

    fn validate_task(&self, task: &BenchmarkTask) -> Result<(), String> {
        if task.prompt.is_empty() {
            return Err(format!("{}: 缺少 prompt 字段", task.instance_id));
        }
        Ok(())
    }

    fn build_user_prompt(&self, task: &BenchmarkTask) -> String {
        task.prompt.clone()
    }

    fn setup_workspace(
        &self,
        _task: &BenchmarkTask,
        base_work_dir: &Path,
    ) -> Result<PathBuf, String> {
        Ok(base_work_dir.to_path_buf())
    }

    fn extract_result(
        &self,
        task: &BenchmarkTask,
        raw_reply: Option<&str>,
        _work_dir: &Path,
        status: TaskStatus,
        metrics: super::metrics::TaskMetrics,
        model_name: &str,
        error: Option<String>,
    ) -> BenchmarkResult {
        BenchmarkResult {
            instance_id: task.instance_id.clone(),
            benchmark: self.kind().as_str().to_string(),
            status,
            raw_reply: raw_reply.map(|s| s.to_string()),
            model_patch: None,
            final_answer: None,
            completion: None,
            metrics,
            model_name_or_path: model_name.to_string(),
            error,
        }
    }
}

// ---------------------------------------------------------------------------
// 工厂函数
// ---------------------------------------------------------------------------

pub fn create_adapter(kind: BenchmarkKind) -> Box<dyn BenchmarkAdapter> {
    match kind {
        BenchmarkKind::SweBench => Box::new(SweBenchAdapter),
        BenchmarkKind::Gaia => Box::new(GaiaAdapter),
        BenchmarkKind::HumanEval => Box::new(HumanEvalAdapter),
        BenchmarkKind::Generic => Box::new(GenericAdapter),
    }
}

// ---------------------------------------------------------------------------
// 内部 git 辅助
// ---------------------------------------------------------------------------

fn run_git_cmd(dir: &Path, args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("执行 git {} 失败: {e}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {} 失败: {stderr}", args.join(" ")));
    }
    Ok(())
}

fn run_git_cmd_in(cwd: &Path, args: &[&str]) -> Result<(), String> {
    run_git_cmd(cwd, args)
}

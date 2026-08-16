//! Benchmark 适配器：为每种 benchmark 定义输入解析、工作区 setup、system prompt 定制和结果提取。

use crate::cm_benchmark::artifact;
use crate::cm_benchmark::metrics::TaskMetrics;
use crate::cm_benchmark::types::{BenchmarkKind, BenchmarkResult, BenchmarkTask, TaskStatus};
use std::path::{Path, PathBuf};

/// 适配器统一接口：每种 benchmark 实现此 trait。
#[allow(clippy::too_many_arguments)]
pub trait BenchmarkAdapter: Send + Sync {
    fn kind(&self) -> BenchmarkKind;

    fn validate_task(&self, task: &BenchmarkTask) -> Result<(), String>;

    fn build_user_prompt(&self, task: &BenchmarkTask) -> String;

    fn system_prompt_suffix(&self) -> Option<String> {
        None
    }

    fn setup_workspace(
        &self,
        task: &BenchmarkTask,
        base_work_dir: &Path,
    ) -> Result<PathBuf, String>;

    fn extract_result(
        &self,
        task: &BenchmarkTask,
        raw_reply: Option<&str>,
        work_dir: &Path,
        status: TaskStatus,
        metrics: TaskMetrics,
        model_name: &str,
        error: Option<String>,
    ) -> BenchmarkResult;

    /// 尝试修复结束后残留的子进程（各 benchmark 定制），默认空操作。
    fn cleanup(&self, _work_dir: &Path) {}
}

// ---------------------------------------------------------------------------
// 默认实现：SweBench / GAIA / HumanEval / Generic
// ---------------------------------------------------------------------------

struct SweBenchAdapter;
struct GaiaAdapter;
struct HumanEvalAdapter;

/// 覆盖默认编程工作台「先问清楚 / 未授权不改代码」的产品语气。
const HUMAN_EVAL_SYSTEM_SUFFIX: &str = "\
You are completing a HumanEval-style Python function. Do not ask clarifying questions. \
Do not wait for extra authorization. Do not call tools. Output only the code completion.";

/// 官方 harness 将本前缀与模型 `completion` 拼接后执行；须让模型续写函数体。
const HUMAN_EVAL_USER_INSTRUCTION: &str = "\
Complete the Python function below. Do not ask what to do. Do not explain. \
Emit only the continuation that is concatenated after this prefix (typically the indented body). \
A ```python fence is allowed.\n\n";

fn humaneval_user_prompt(stub: &str) -> String {
    let mut out = String::with_capacity(HUMAN_EVAL_USER_INSTRUCTION.len() + stub.len());
    out.push_str(HUMAN_EVAL_USER_INSTRUCTION);
    out.push_str(stub);
    out
}

impl BenchmarkAdapter for SweBenchAdapter {
    fn kind(&self) -> BenchmarkKind {
        BenchmarkKind::SweBench
    }

    fn validate_task(&self, task: &BenchmarkTask) -> Result<(), String> {
        if task.instance_id.is_empty() {
            return Err("instance_id 为空".to_string());
        }
        if task.repo.is_none() {
            return Err("SWE-bench 任务缺少 repo".to_string());
        }
        if task.base_commit.is_none() {
            return Err("SWE-bench 任务缺少 base_commit".to_string());
        }
        if task.problem_statement.is_none() {
            return Err("SWE-bench 任务缺少 problem_statement".to_string());
        }
        Ok(())
    }

    fn build_user_prompt(&self, task: &BenchmarkTask) -> String {
        let mut prompt = String::new();
        if let Some(ref ps) = task.problem_statement {
            prompt.push_str(ps);
        }
        if let Some(ref hints) = task.hints_text {
            prompt.push_str("\n\n提示：");
            prompt.push_str(hints);
        }
        prompt
    }

    fn setup_workspace(
        &self,
        task: &BenchmarkTask,
        base_work_dir: &Path,
    ) -> Result<PathBuf, String> {
        let repo = task
            .repo
            .as_ref()
            .ok_or_else(|| "SWE-bench 任务缺少 repo".to_string())?;
        let commit = task
            .base_commit
            .as_ref()
            .ok_or_else(|| "SWE-bench 任务缺少 base_commit".to_string())?;

        let repo_dir = base_work_dir.join(repo.split('/').next_back().unwrap_or(repo));

        if !repo_dir.exists() {
            let url = format!("https://github.com/{repo}.git");
            let status = std::process::Command::new("git")
                .args(["clone", "--depth=1", &url])
                .current_dir(base_work_dir)
                .status()
                .map_err(|e| format!("git clone 失败: {e}"))?;
            if !status.success() {
                return Err("git clone 返回非零".to_string());
            }
        }

        let status = std::process::Command::new("git")
            .args(["checkout", "--force", commit])
            .current_dir(&repo_dir)
            .status()
            .map_err(|e| format!("git checkout 失败: {e}"))?;
        if !status.success() {
            return Err(format!("git checkout {} 失败", commit));
        }

        Ok(repo_dir)
    }

    fn extract_result(
        &self,
        task: &BenchmarkTask,
        raw_reply: Option<&str>,
        work_dir: &Path,
        status: TaskStatus,
        metrics: TaskMetrics,
        model_name: &str,
        error: Option<String>,
    ) -> BenchmarkResult {
        let patch = if status == TaskStatus::Success {
            artifact::extract_git_patch(work_dir).ok()
        } else {
            None
        };
        BenchmarkResult {
            instance_id: task.instance_id.clone(),
            sample_index: 0,
            benchmark: "swe_bench".to_string(),
            status,
            raw_reply: raw_reply.map(String::from),
            model_patch: patch,
            final_answer: None,
            completion: None,
            metrics,
            model_name_or_path: model_name.to_string(),
            error,
        }
    }
}

impl BenchmarkAdapter for GaiaAdapter {
    fn kind(&self) -> BenchmarkKind {
        BenchmarkKind::Gaia
    }

    fn validate_task(&self, task: &BenchmarkTask) -> Result<(), String> {
        if task.instance_id.is_empty() {
            return Err("instance_id 为空".to_string());
        }
        if task.prompt.is_empty() {
            return Err("GAIA 任务缺少 prompt (question)".to_string());
        }
        Ok(())
    }

    fn build_user_prompt(&self, task: &BenchmarkTask) -> String {
        if task.file_attachments.is_empty() {
            task.prompt.clone()
        } else {
            format!(
                "{}\n\n附件：{}",
                task.prompt,
                task.file_attachments.join(", ")
            )
        }
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
        metrics: TaskMetrics,
        model_name: &str,
        error: Option<String>,
    ) -> BenchmarkResult {
        let answer = raw_reply.and_then(artifact::extract_final_answer);
        BenchmarkResult {
            instance_id: task.instance_id.clone(),
            sample_index: 0,
            benchmark: "gaia".to_string(),
            status,
            raw_reply: raw_reply.map(String::from),
            model_patch: None,
            final_answer: answer,
            completion: None,
            metrics,
            model_name_or_path: model_name.to_string(),
            error,
        }
    }
}

impl BenchmarkAdapter for HumanEvalAdapter {
    fn kind(&self) -> BenchmarkKind {
        BenchmarkKind::HumanEval
    }

    fn validate_task(&self, task: &BenchmarkTask) -> Result<(), String> {
        if task.instance_id.is_empty() {
            return Err("instance_id 为空".to_string());
        }
        if task.prompt.is_empty() {
            return Err("HumanEval 任务缺少 prompt".to_string());
        }
        Ok(())
    }

    fn build_user_prompt(&self, task: &BenchmarkTask) -> String {
        humaneval_user_prompt(&task.prompt)
    }

    fn system_prompt_suffix(&self) -> Option<String> {
        Some(HUMAN_EVAL_SYSTEM_SUFFIX.to_string())
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
        metrics: TaskMetrics,
        model_name: &str,
        error: Option<String>,
    ) -> BenchmarkResult {
        let code =
            raw_reply.map(|reply| artifact::extract_humaneval_completion(reply, &task.prompt));
        BenchmarkResult {
            instance_id: task.instance_id.clone(),
            sample_index: 0,
            benchmark: "human_eval".to_string(),
            status,
            raw_reply: raw_reply.map(String::from),
            model_patch: None,
            final_answer: None,
            completion: code,
            metrics,
            model_name_or_path: model_name.to_string(),
            error,
        }
    }
}

struct GenericAdapter;

impl BenchmarkAdapter for GenericAdapter {
    fn kind(&self) -> BenchmarkKind {
        BenchmarkKind::Generic
    }

    fn validate_task(&self, task: &BenchmarkTask) -> Result<(), String> {
        if task.instance_id.is_empty() {
            return Err("instance_id 为空".to_string());
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
        metrics: TaskMetrics,
        model_name: &str,
        error: Option<String>,
    ) -> BenchmarkResult {
        BenchmarkResult {
            instance_id: task.instance_id.clone(),
            sample_index: 0,
            benchmark: "generic".to_string(),
            status,
            raw_reply: raw_reply.map(String::from),
            model_patch: None,
            final_answer: None,
            completion: None,
            metrics,
            model_name_or_path: model_name.to_string(),
            error,
        }
    }
}

/// 根据 `BenchmarkKind` 创建对应适配器实例。
pub fn create_adapter(kind: BenchmarkKind) -> Box<dyn BenchmarkAdapter> {
    match kind {
        BenchmarkKind::SweBench => Box::new(SweBenchAdapter),
        BenchmarkKind::Gaia => Box::new(GaiaAdapter),
        BenchmarkKind::HumanEval => Box::new(HumanEvalAdapter),
        BenchmarkKind::Generic => Box::new(GenericAdapter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn humaneval_sample_task() -> BenchmarkTask {
        serde_json::from_str(
            r#"{"instance_id":"tiny/add","prompt":"def add(a, b):\n    \"\"\"Add two numbers.\"\"\"\n"}"#,
        )
        .expect("sample HumanEval task JSON")
    }

    #[test]
    fn humaneval_user_prompt_includes_stub_and_completion_instruction() {
        let adapter = create_adapter(BenchmarkKind::HumanEval);
        let task = humaneval_sample_task();
        let user = adapter.build_user_prompt(&task);
        assert!(user.contains("def add(a, b):"));
        assert!(user.contains("Do not ask what to do"));
        assert!(user.starts_with(HUMAN_EVAL_USER_INSTRUCTION));
    }

    #[test]
    fn humaneval_system_suffix_overrides_conversational_defaults() {
        let adapter = create_adapter(BenchmarkKind::HumanEval);
        let suffix = adapter.system_prompt_suffix().expect("suffix");
        assert!(suffix.contains("Do not ask clarifying questions"));
        assert!(suffix.contains("Do not call tools"));
    }
}

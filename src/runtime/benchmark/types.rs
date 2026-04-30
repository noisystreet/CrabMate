//! Benchmark 共享类型：任务输入、任务结果、运行配置。

use serde::{Deserialize, Serialize};

/// 支持的 benchmark 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkKind {
    SweBench,
    Gaia,
    HumanEval,
    /// 通用模式：仅以 `prompt` 字段发送给 agent，收集自由文本回复。
    Generic,
}

impl BenchmarkKind {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "swe_bench" | "swebench" => Ok(Self::SweBench),
            "gaia" => Ok(Self::Gaia),
            "human_eval" | "humaneval" => Ok(Self::HumanEval),
            "generic" => Ok(Self::Generic),
            _ => Err(format!(
                "未知 benchmark 类型: {s:?}（支持 swe_bench、gaia、human_eval、generic）"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SweBench => "swe_bench",
            Self::Gaia => "gaia",
            Self::HumanEval => "human_eval",
            Self::Generic => "generic",
        }
    }
}

// ---------------------------------------------------------------------------
// 通用任务输入（JSONL 反序列化）
// ---------------------------------------------------------------------------

/// 从 JSONL 文件中读取的单条 benchmark 任务。
///
/// 各 benchmark 有自己的特有字段，但共享 `instance_id` 和 `prompt`（或等价文案）。
/// 特有字段以 `Option` 存放，adapter 负责校验必填性。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkTask {
    pub instance_id: String,

    /// 通用提示文本（GAIA 的 question、HumanEval 的 prompt、Generic 直接使用）。
    #[serde(default)]
    pub prompt: String,

    // -- SWE-bench 特有 --
    /// GitHub 仓库（如 "django/django"）
    #[serde(default)]
    pub repo: Option<String>,
    /// 基线提交 SHA
    #[serde(default)]
    pub base_commit: Option<String>,
    /// Issue / PR 问题描述
    #[serde(default)]
    pub problem_statement: Option<String>,
    /// 可选提示
    #[serde(default)]
    pub hints_text: Option<String>,

    // -- GAIA 特有 --
    /// 附件文件路径列表
    #[serde(default)]
    pub file_attachments: Vec<String>,

    // -- HumanEval 特有 --
    /// 原始 task_id（如 "HumanEval/0"）；与 instance_id 可相同
    #[serde(default)]
    pub task_id: Option<String>,
    /// 函数入口点（可选，adapter 中通过 prompt 传递）
    #[serde(default)]
    pub entry_point: Option<String>,
    /// HumanEval 官方 `test` 字段（单元测试源码），供外挂 Python 判分；`bench` 运行时忽略。
    #[serde(
        default,
        rename = "humaneval_test",
        skip_serializing_if = "Option::is_none"
    )]
    pub humaneval_test: Option<String>,
}

// ---------------------------------------------------------------------------
// 任务结果输出
// ---------------------------------------------------------------------------

/// 单条任务的执行结果（写入输出 JSONL）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub instance_id: String,
    pub benchmark: String,

    /// 执行状态
    pub status: TaskStatus,

    /// Agent 的原始回复文本
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_reply: Option<String>,

    /// 提取的产物（patch / answer / code 等，由 adapter 决定字段名）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_patch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_answer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,

    /// 指标
    pub metrics: super::metrics::TaskMetrics,

    /// 使用的模型名称
    pub model_name_or_path: String,

    /// 如有错误，记录错误消息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 单条任务的终态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Success,
    Timeout,
    Error,
    MaxRounds,
}

// ---------------------------------------------------------------------------
// Batch 运行配置
// ---------------------------------------------------------------------------

/// 从 CLI 传入的 batch 运行参数。
#[derive(Debug, Clone)]
pub struct BatchRunConfig {
    pub benchmark: BenchmarkKind,
    pub input_path: String,
    pub output_path: String,
    /// 每个任务的全局超时（秒），0 = 不限制
    pub task_timeout_secs: u64,
    /// 每个任务最大 agent 工具调用轮次，0 = 不限制
    pub max_tool_rounds: usize,
    /// 是否续跑（跳过已有结果的 instance_id）
    pub resume_from_existing: bool,
    /// 自定义 system prompt 覆盖
    pub system_prompt_override: Option<String>,
}

/// 将 JSONL 中的一行解析为 `BenchmarkTask`。
///
/// 空行与以 `#` 开头的注释行返回 `Ok(None)`；其余行按 JSON 反序列化。
pub fn parse_task_jsonl_line(line: &str) -> Result<Option<BenchmarkTask>, serde_json::Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Ok(None);
    }
    serde_json::from_str(trimmed).map(Some)
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn benchmark_kind_parse_aliases() {
        assert_eq!(
            BenchmarkKind::parse("human_eval").unwrap(),
            BenchmarkKind::HumanEval
        );
        assert_eq!(
            BenchmarkKind::parse("HumanEval").unwrap(),
            BenchmarkKind::HumanEval
        );
        assert_eq!(
            BenchmarkKind::parse("humaneval").unwrap(),
            BenchmarkKind::HumanEval
        );
        assert_eq!(
            BenchmarkKind::parse("swe-bench").unwrap(),
            BenchmarkKind::SweBench
        );
        assert_eq!(
            BenchmarkKind::parse("SWE_BENCH").unwrap(),
            BenchmarkKind::SweBench
        );
        let err = BenchmarkKind::parse("unknown_suite").unwrap_err();
        assert!(err.contains("未知 benchmark 类型"), "{err}");
    }

    #[test]
    fn parse_task_jsonl_skips_blank_and_hash() {
        assert!(parse_task_jsonl_line("").unwrap().is_none());
        assert!(parse_task_jsonl_line("   ").unwrap().is_none());
        assert!(parse_task_jsonl_line("# comment").unwrap().is_none());
        assert!(parse_task_jsonl_line(" # leading").unwrap().is_none());
    }

    #[test]
    fn humaneval_task_roundtrip_jsonl() {
        let line = r#"{"instance_id":"t0","prompt":"def f():\n    \"\"\"x\"\"\"\n","task_id":"HumanEval/0","entry_point":"f","humaneval_test":"def check(x):\n    pass\n"}"#;
        let t = parse_task_jsonl_line(line).unwrap().expect("task");
        assert_eq!(t.instance_id, "t0");
        assert_eq!(t.entry_point.as_deref(), Some("f"));
        assert!(t.humaneval_test.as_deref().unwrap().contains("check"));
    }
}

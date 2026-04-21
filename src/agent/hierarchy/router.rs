//! 路由层：根据任务复杂度选择执行模式
//!
//! 提供两种路由决策方式：
//! - 快速规则路由（默认）：基于关键词匹配，无需 LLM 调用
//! - 智能 LLM 路由：使用 LLM 分析任务语义，提供更精准的路由决策

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{CompleteChatRetryingParams, LlmRetryingTransportOpts, complete_chat_retrying};
use crate::types::{Message, message_content_as_str};

use super::task::ExecutionStrategy;

/// Agent 执行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentMode {
    /// 单一 Agent（现有默认模式）
    #[default]
    Single,
    /// 分层架构（Manager + Operator）
    Hierarchical,
    /// 多 Agent 群体
    MultiAgent,
    /// 纯 ReAct
    ReAct,
}

impl AgentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentMode::Single => "single",
            AgentMode::Hierarchical => "hierarchical",
            AgentMode::MultiAgent => "multi_agent",
            AgentMode::ReAct => "react",
        }
    }
}

/// 任务复杂度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskComplexity {
    Simple,      // 1-2 步
    Medium,      // 3-5 步
    Complex,     // 6-20 步
    VeryComplex, // 20+ 步
}

/// 路由决策结果
#[derive(Debug, Clone)]
pub struct RouterOutput {
    pub mode: AgentMode,
    pub max_iterations: usize,
    pub max_sub_goals: usize,
    pub execution_strategy: ExecutionStrategy,
    /// 路由决策理由（用于调试和可观测性）
    pub reasoning: Option<String>,
    /// 使用的路由策略
    pub routing_strategy: RoutingStrategy,
}

impl Default for RouterOutput {
    fn default() -> Self {
        Self {
            mode: AgentMode::Single,
            max_iterations: 10,
            max_sub_goals: 10,
            execution_strategy: ExecutionStrategy::Hybrid,
            reasoning: None,
            routing_strategy: RoutingStrategy::RuleBased,
        }
    }
}

/// 路由策略类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingStrategy {
    /// 基于规则的快速路由
    RuleBased,
    /// 基于 LLM 的智能路由
    LlmBased,
    /// 用户显式指定
    UserOverride,
    /// 基于历史数据的推荐
    HistoryBased,
}

/// 历史执行记录
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ExecutionRecord {
    /// 任务特征（哈希或关键词）
    task_signature: String,
    /// 执行模式
    mode: AgentMode,
    /// 执行结果（成功/失败）
    success: bool,
    /// 执行耗时
    duration_ms: u64,
    /// 记录时间
    timestamp: Instant,
}

/// 智能路由器
pub struct SmartRouter {
    /// 历史执行缓存
    history_cache: Arc<Mutex<HashMap<String, Vec<ExecutionRecord>>>>,
    /// 缓存最大条目数
    max_history_entries: usize,
    /// 历史记录过期时间
    history_ttl: Duration,
}

impl Default for SmartRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl SmartRouter {
    /// 创建新的智能路由器
    pub fn new() -> Self {
        Self {
            history_cache: Arc::new(Mutex::new(HashMap::new())),
            max_history_entries: 1000,
            history_ttl: Duration::from_secs(3600 * 24), // 24小时过期
        }
    }

    /// 使用规则快速路由（同步，无 LLM 调用）
    pub fn route_with_rules(task: &str) -> RouterOutput {
        let complexity = Self::estimate_complexity_with_rules(task);
        let task_preview = truncate_string(task, 80);
        log::info!(
            target: "crabmate",
            "[ROUTER] Rule-based routing: complexity={:?} task={}",
            complexity,
            task_preview
        );

        match complexity {
            TaskComplexity::Simple => RouterOutput {
                mode: AgentMode::Single,
                max_iterations: 5,
                max_sub_goals: 3,
                execution_strategy: ExecutionStrategy::Sequential,
                reasoning: Some("简单任务，使用单一 Agent 模式".to_string()),
                routing_strategy: RoutingStrategy::RuleBased,
            },
            TaskComplexity::Medium => RouterOutput {
                mode: AgentMode::ReAct,
                max_iterations: 10,
                max_sub_goals: 5,
                execution_strategy: ExecutionStrategy::Hybrid,
                reasoning: Some("中等复杂度任务，使用 ReAct 模式".to_string()),
                routing_strategy: RoutingStrategy::RuleBased,
            },
            TaskComplexity::Complex => RouterOutput {
                mode: AgentMode::Hierarchical,
                max_iterations: 30,
                max_sub_goals: 20,
                execution_strategy: ExecutionStrategy::Hybrid,
                reasoning: Some("复杂任务，使用分层架构".to_string()),
                routing_strategy: RoutingStrategy::RuleBased,
            },
            TaskComplexity::VeryComplex => RouterOutput {
                mode: AgentMode::MultiAgent,
                max_iterations: 50,
                max_sub_goals: 50,
                execution_strategy: ExecutionStrategy::Parallel,
                reasoning: Some("非常复杂任务，使用多 Agent 并行模式".to_string()),
                routing_strategy: RoutingStrategy::RuleBased,
            },
        }
    }

    /// 使用 LLM 进行智能路由决策（异步）
    #[allow(clippy::too_many_arguments)]
    pub async fn route_with_llm(
        &self,
        task: &str,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<RouterOutput, RouterError> {
        let start_time = Instant::now();
        let prompt = self.build_routing_prompt(task);

        let messages = vec![Message::user_only(&prompt)];
        let request = crate::types::ChatRequest {
            model: cfg.model.clone(),
            messages,
            stream: Some(false),
            temperature: 0.1, // 低温度，确保确定性
            max_tokens: 500,
            tools: None,
            tool_choice: None,
            seed: None,
            reasoning_split: Some(false),
            thinking: None,
        };

        let params = CompleteChatRetryingParams::new(
            llm_backend,
            client,
            api_key,
            cfg,
            LlmRetryingTransportOpts::headless_no_stream(),
            None,
            None,
        );

        match complete_chat_retrying(&params, &request).await {
            Ok((response, _)) => {
                let content = message_content_as_str(&response.content)
                    .unwrap_or("")
                    .trim()
                    .to_string();

                let llm_duration = start_time.elapsed();
                log::info!(
                    target: "crabmate",
                    "[ROUTER] LLM routing completed in {:?}",
                    llm_duration
                );

                self.parse_llm_routing_response(&content, task)
            }
            Err(e) => {
                log::warn!(
                    target: "crabmate",
                    "[ROUTER] LLM routing failed: {}, falling back to rule-based",
                    e
                );
                // LLM 路由失败时，降级到规则路由
                Ok(Self::route_with_rules(task))
            }
        }
    }

    /// 智能路由：优先使用历史数据，其次 LLM，最后规则
    #[allow(clippy::too_many_arguments)]
    pub async fn route_smart(
        &self,
        task: &str,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
        use_llm: bool,
    ) -> RouterOutput {
        // 1. 首先检查历史缓存
        if let Some(record) = self.find_similar_task(task) {
            log::info!(
                target: "crabmate",
                "[ROUTER] Found similar task in history, using mode={:?}",
                record.mode
            );
            return Self::output_from_mode(
                record.mode,
                "基于历史相似任务推荐".to_string(),
                RoutingStrategy::HistoryBased,
            );
        }

        // 2. 如果启用 LLM 且任务较复杂，使用 LLM 路由
        if use_llm && self.should_use_llm_routing(task) {
            match self
                .route_with_llm(task, cfg, llm_backend, client, api_key)
                .await
            {
                Ok(output) => return output,
                Err(_) => {
                    // LLM 路由失败，继续降级到规则路由
                }
            }
        }

        // 3. 默认使用规则路由
        Self::route_with_rules(task)
    }

    /// 用户显式指定执行模式
    pub fn route_with_override(task: &str, mode: AgentMode) -> RouterOutput {
        log::info!(
            target: "crabmate",
            "[ROUTER] User override mode={:?} for task={}",
            mode,
            truncate_string(task, 50)
        );

        let reasoning = format!("用户显式指定 {:?} 模式", mode);
        Self::output_from_mode(mode, reasoning, RoutingStrategy::UserOverride)
    }

    /// 记录执行结果到历史缓存
    pub fn record_execution(&self, task: &str, mode: AgentMode, success: bool, duration_ms: u64) {
        let signature = Self::compute_task_signature(task);
        let record = ExecutionRecord {
            task_signature: signature.clone(),
            mode,
            success,
            duration_ms,
            timestamp: Instant::now(),
        };

        let mut cache = self.history_cache.lock().unwrap();
        let entries = cache.entry(signature).or_default();

        // 添加新记录
        entries.push(record);

        // 清理过期记录
        entries.retain(|r| r.timestamp.elapsed() < self.history_ttl);

        // 限制条目数
        if entries.len() > self.max_history_entries {
            entries.remove(0);
        }

        log::debug!(
            target: "crabmate",
            "[ROUTER] Recorded execution: mode={:?}, success={}",
            mode,
            success
        );
    }

    /// 构建路由决策 Prompt
    fn build_routing_prompt(&self, task: &str) -> String {
        format!(
            r#"你是一个任务路由专家。请分析以下任务，决定最适合的执行模式。

## 任务描述
{}

## 可选执行模式

1. **single** - 单一 Agent 模式
   - 适用：简单、单步任务（读取文件、简单查询）
   - 特点：直接执行，无复杂规划

2. **react** - ReAct 模式
   - 适用：中等复杂度，需要多步推理（分析、搜索、简单修改）
   - 特点：思考-行动-观察循环

3. **hierarchical** - 分层架构（Manager + Operator）
   - 适用：复杂任务，需要分解为多个子目标（代码重构、多文件修改、测试覆盖）
   - 特点：任务分解、协调执行

4. **multi_agent** - 多 Agent 并行
   - 适用：非常复杂，可并行化的任务（大规模重构、多模块同时修改）
   - 特点：多个 Agent 并行工作

## 输出格式
请输出 JSON 格式：
{{
  "mode": "single|react|hierarchical|multi_agent",
  "reasoning": "简要说明选择该模式的理由",
  "estimated_steps": 数字估计（1-50）
}}

只输出 JSON，不要其他解释。"#,
            task
        )
    }

    /// 解析 LLM 路由响应
    fn parse_llm_routing_response(
        &self,
        content: &str,
        _task: &str,
    ) -> Result<RouterOutput, RouterError> {
        #[derive(serde::Deserialize)]
        struct LlmRoutingResponse {
            mode: String,
            reasoning: String,
            #[allow(dead_code)]
            estimated_steps: Option<usize>,
        }

        let parsed: LlmRoutingResponse = serde_json::from_str(content)
            .map_err(|e| RouterError::ParseError(format!("Failed to parse LLM response: {}", e)))?;

        let mode = match parsed.mode.as_str() {
            "single" => AgentMode::Single,
            "react" => AgentMode::ReAct,
            "hierarchical" => AgentMode::Hierarchical,
            "multi_agent" => AgentMode::MultiAgent,
            _ => {
                log::warn!(
                    target: "crabmate",
                    "[ROUTER] Unknown mode from LLM: {}, using default",
                    parsed.mode
                );
                AgentMode::Single
            }
        };

        Ok(RouterOutput {
            mode,
            max_iterations: Self::iterations_for_mode(mode),
            max_sub_goals: Self::sub_goals_for_mode(mode),
            execution_strategy: Self::strategy_for_mode(mode),
            reasoning: Some(parsed.reasoning),
            routing_strategy: RoutingStrategy::LlmBased,
        })
    }

    /// 基于规则估算复杂度
    fn estimate_complexity_with_rules(task: &str) -> TaskComplexity {
        let task_lower = task.to_lowercase();
        let mut score = 0usize;

        // 关键词评估
        let analysis_keywords = ["分析", "比较", "评估", "调研"];
        for kw in analysis_keywords {
            if task_lower.contains(kw) {
                score += 2;
            }
        }

        let parallel_keywords = ["多个", "并行", "同时", "分别"];
        for kw in parallel_keywords {
            if task_lower.contains(kw) {
                score += 3;
            }
        }

        let complex_keywords = ["测试", "修改", "重构", "迁移", "部署"];
        for kw in complex_keywords {
            if task_lower.contains(kw) {
                score += 2;
            }
        }

        // 工具需求预估
        let tool_keywords = [
            "文件",
            "代码",
            "测试",
            "编译",
            "部署",
            "API",
            "数据库",
            "配置",
        ];
        for kw in tool_keywords {
            if task_lower.contains(kw) {
                score += 1;
            }
        }

        // 步骤数量预估
        let step_indicators = ["1.", "2.", "3.", "首先", "然后", "接着", "最后"];
        for indicator in step_indicators {
            if task_lower.contains(indicator) {
                score += 1;
            }
        }

        match score {
            0..=2 => TaskComplexity::Simple,
            3..=5 => TaskComplexity::Medium,
            6..=10 => TaskComplexity::Complex,
            _ => TaskComplexity::VeryComplex,
        }
    }

    /// 计算任务特征签名
    fn compute_task_signature(task: &str) -> String {
        // 简化版：提取关键词并哈希
        let keywords: Vec<&str> = task
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .take(10)
            .collect();
        keywords.join("_")
    }

    /// 在历史缓存中查找相似任务
    fn find_similar_task(&self, task: &str) -> Option<ExecutionRecord> {
        let signature = Self::compute_task_signature(task);
        let cache = self.history_cache.lock().unwrap();

        if let Some(entries) = cache.get(&signature) {
            // 找到最近的成功记录
            return entries
                .iter()
                .filter(|r| r.success && r.timestamp.elapsed() < self.history_ttl)
                .max_by_key(|r| r.timestamp)
                .cloned();
        }

        None
    }

    /// 判断是否应使用 LLM 路由
    fn should_use_llm_routing(&self, task: &str) -> bool {
        // 任务长度超过阈值或包含复杂关键词时使用 LLM
        let task_lower = task.to_lowercase();
        let complex_indicators = [
            "重构",
            "架构",
            "设计",
            "优化",
            "性能",
            "refactor",
            "architecture",
            "design",
            "optimize",
            "performance",
        ];

        task.len() > 50 || complex_indicators.iter().any(|&kw| task_lower.contains(kw))
    }

    /// 根据模式生成输出
    fn output_from_mode(
        mode: AgentMode,
        reasoning: String,
        strategy: RoutingStrategy,
    ) -> RouterOutput {
        RouterOutput {
            mode,
            max_iterations: Self::iterations_for_mode(mode),
            max_sub_goals: Self::sub_goals_for_mode(mode),
            execution_strategy: Self::strategy_for_mode(mode),
            reasoning: Some(reasoning),
            routing_strategy: strategy,
        }
    }

    /// 获取模式对应的最大迭代次数
    fn iterations_for_mode(mode: AgentMode) -> usize {
        match mode {
            AgentMode::Single => 5,
            AgentMode::ReAct => 10,
            AgentMode::Hierarchical => 30,
            AgentMode::MultiAgent => 50,
        }
    }

    /// 获取模式对应的最大子目标数
    fn sub_goals_for_mode(mode: AgentMode) -> usize {
        match mode {
            AgentMode::Single => 3,
            AgentMode::ReAct => 5,
            AgentMode::Hierarchical => 20,
            AgentMode::MultiAgent => 50,
        }
    }

    /// 获取模式对应的执行策略
    fn strategy_for_mode(mode: AgentMode) -> ExecutionStrategy {
        match mode {
            AgentMode::Single => ExecutionStrategy::Sequential,
            AgentMode::ReAct => ExecutionStrategy::Hybrid,
            AgentMode::Hierarchical => ExecutionStrategy::Hybrid,
            AgentMode::MultiAgent => ExecutionStrategy::Parallel,
        }
    }
}

/// 路由器（向后兼容的静态方法）
pub struct Router;

impl Router {
    /// 根据任务内容进行路由决策（向后兼容，使用规则路由）
    pub fn route(task: &str) -> RouterOutput {
        SmartRouter::route_with_rules(task)
    }

    /// 使用 LLM 进行智能路由
    pub async fn route_with_llm(
        task: &str,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<RouterOutput, RouterError> {
        let router = SmartRouter::new();
        router
            .route_with_llm(task, cfg, llm_backend, client, api_key)
            .await
    }

    /// 用户显式指定模式
    pub fn route_with_override(task: &str, mode: AgentMode) -> RouterOutput {
        SmartRouter::route_with_override(task, mode)
    }
}

/// 路由错误
#[derive(Debug)]
pub enum RouterError {
    LlmError(String),
    ParseError(String),
}

impl std::fmt::Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterError::LlmError(s) => write!(f, "LLM error: {}", s),
            RouterError::ParseError(s) => write!(f, "Parse error: {}", s),
        }
    }
}

impl std::error::Error for RouterError {}

/// 截断字符串到指定长度（按字符边界截断，支持中文）
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated = s
            .char_indices()
            .take(max_len.saturating_sub(3))
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &s[..truncated])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_task() {
        let output = SmartRouter::route_with_rules("帮我读取 /tmp/test.txt 文件内容");
        assert_eq!(output.mode, AgentMode::Single);
        assert_eq!(output.routing_strategy, RoutingStrategy::RuleBased);
    }

    #[test]
    fn test_complex_task() {
        let output = SmartRouter::route_with_rules("读取代码并分析测试覆盖率");
        assert_eq!(output.mode, AgentMode::Hierarchical);
    }

    #[test]
    fn test_user_override() {
        let output = SmartRouter::route_with_override("简单任务", AgentMode::Hierarchical);
        assert_eq!(output.mode, AgentMode::Hierarchical);
        assert_eq!(output.routing_strategy, RoutingStrategy::UserOverride);
    }

    #[test]
    fn test_backward_compatibility() {
        // 测试向后兼容
        let output = Router::route("帮我读取 /tmp/test.txt 文件内容");
        assert_eq!(output.mode, AgentMode::Single);
    }
}

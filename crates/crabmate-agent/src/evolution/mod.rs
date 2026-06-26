//! Agent 自我进化模块：决策历史记录、策略分析、system prompt 动态注入。

mod history_logger;
mod prompt_injector;
mod strategy_analyzer;

pub use history_logger::DecisionHistoryLogger;
pub use prompt_injector::PromptInjector;
pub use strategy_analyzer::{StrategyAnalyzer, StrategyHint};

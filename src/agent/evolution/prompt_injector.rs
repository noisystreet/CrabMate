//! System Prompt 动态注入器：将策略建议转为模型可理解的指令。

use super::strategy_analyzer::StrategyHint;
use crate::types::{Message, message_content_get_or_insert_empty_text};

#[cfg(test)]
use crate::types::{MessageContent, message_content_as_str};

/// System Prompt 注入器
pub struct PromptInjector {
    /// 是否启用动态注入
    enabled: bool,
    /// 策略建议缓存
    current_hints: Vec<StrategyHint>,
}

impl PromptInjector {
    /// 创建新的注入器
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            current_hints: Vec::new(),
        }
    }

    /// 更新当前策略建议
    pub fn update_hints(&mut self, hints: Vec<StrategyHint>) {
        self.current_hints = hints;
    }

    /// 生成注入文本（附加到 system prompt）
    pub fn generate_injection(&self) -> Option<String> {
        if !self.enabled || self.current_hints.is_empty() {
            return None;
        }

        let hints_text: Vec<String> = self
            .current_hints
            .iter()
            .map(|h| h.to_prompt_text())
            .collect();

        Some(format!(
            "\n\n## 当前会话行为建议（供参考）\n{}\n",
            hints_text.join("\n")
        ))
    }

    /// 将策略建议注入到消息列表的 system 消息中
    pub fn inject_into_messages(&self, messages: &mut Vec<Message>) -> bool {
        let Some(injection) = self.generate_injection() else {
            return false;
        };

        // 查找或创建 system 消息
        let system_msg = messages.iter_mut().find(|m| m.role == "system");

        if let Some(msg) = system_msg {
            // 追加到现有 system 消息
            let current = message_content_get_or_insert_empty_text(&mut msg.content);
            current.push_str(&injection);
            true
        } else {
            // 创建新的 system 消息并插入开头
            messages.insert(
                0,
                Message {
                    role: "system".to_string(),
                    content: Some(injection.into()),
                    reasoning_content: None,
                    reasoning_details: None,
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                },
            );
            true
        }
    }

    /// 清除当前建议
    pub fn clear_hints(&mut self) {
        self.current_hints.clear();
    }

    /// 获取当前建议数量
    pub fn hints_count(&self) -> usize {
        self.current_hints.len()
    }
}

impl Default for PromptInjector {
    fn default() -> Self {
        Self::new(true)
    }
}

/// 构建基础的进化系统提示（无建议时使用）
#[allow(dead_code)] // 预留与策略注入组合；当前动态段见 [`PromptInjector::generate_injection`]
pub fn evolution_system_prompt_base() -> &'static str {
    r#"## Agent 自我进化行为准则

1. **记录决策**：每次使用工具时，简要思考该工具的选择是否最优
2. **从失败中学习**：如果某个工具执行失败，思考失败原因并尝试替代方案
3. **效率意识**：避免重复调用相同工具，优先使用更高效的工具组合
4. **反思重写**：如果需要多次反思重写，思考是否因为计划不够充分
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_injection_generation() {
        let mut injector = PromptInjector::new(true);
        injector.update_hints(vec![StrategyHint {
            dimension: "test",
            suggestion: "这是测试建议".to_string(),
            confidence: 0.8,
        }]);

        let injection = injector.generate_injection();
        assert!(injection.is_some());
        assert!(injection.unwrap().contains("测试建议"));
    }

    #[test]
    fn test_inject_into_messages() {
        let mut injector = PromptInjector::new(true);
        injector.update_hints(vec![StrategyHint {
            dimension: "tool_selection",
            suggestion: "建议使用语义搜索".to_string(),
            confidence: 0.7,
        }]);

        let mut messages = vec![
            Message {
                role: "user".to_string(),
                content: Some(MessageContent::Text("Hello".to_string())),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some(MessageContent::Text("Hi".to_string())),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];

        let injected = injector.inject_into_messages(&mut messages);
        assert!(injected);

        // 应该有新的 system 消息在开头
        assert_eq!(messages[0].role, "system");
        assert!(
            message_content_as_str(&messages[0].content).is_some_and(|s| s.contains("语义搜索"))
        );
    }

    #[test]
    fn test_disabled_injector() {
        let injector = PromptInjector::new(false);
        let injection = injector.generate_injection();
        assert!(injection.is_none());
    }
}

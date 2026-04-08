//! 决策历史记录器：将每次工具调用的结果写入长期记忆索引。

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// 单次决策记录（简化为可 JSON 序列化的结构）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecord {
    /// 工具名称
    pub tool_name: String,
    /// 工具参数（JSON 字符串）
    pub tool_args: String,
    /// 执行结果摘要（成功/失败）
    pub success: bool,
    /// 反思结论（若有）
    pub reflection_note: Option<String>,
    /// 执行耗时（毫秒）
    pub duration_ms: u64,
    /// 时间戳（Unix epoch 秒）
    pub timestamp: i64,
    /// 反思重写次数
    pub rewrite_count: usize,
}

impl DecisionRecord {
    /// 从工具执行结果构建记录
    pub fn from_tool_result(
        tool_name: &str,
        tool_args: &str,
        success: bool,
        duration_ms: u64,
        rewrite_count: usize,
    ) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            tool_args: tool_args.to_string(),
            success,
            reflection_note: None,
            duration_ms,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            rewrite_count,
        }
    }

    /// 转为用于嵌入的文本片段
    pub fn to_embedding_text(&self) -> String {
        let reflection = self.reflection_note.as_deref().unwrap_or("无反思");
        format!(
            "工具: {} | 参数: {} | 成功: {} | 反思: {} | 耗时: {}ms",
            self.tool_name, self.tool_args, self.success, reflection, self.duration_ms
        )
    }
}

/// 决策历史日志器（进程内聚合）
pub struct DecisionHistoryLogger {
    /// 当前会话的决策记录
    records: Mutex<Vec<DecisionRecord>>,
    /// 当前会话的重写次数
    rewrite_count: Mutex<usize>,
}

impl DecisionHistoryLogger {
    /// 创建新的日志器
    pub fn new() -> Self {
        Self {
            records: Mutex::new(Vec::new()),
            rewrite_count: Mutex::new(0),
        }
    }

    /// 记录一次工具执行
    pub fn log_tool_call(&self, tool_name: &str, tool_args: &str, success: bool, duration_ms: u64) {
        let rewrite_count = *self.rewrite_count.lock().unwrap();
        let record = DecisionRecord::from_tool_result(
            tool_name,
            tool_args,
            success,
            duration_ms,
            rewrite_count,
        );
        self.records.lock().unwrap().push(record);
    }

    /// 反思重写后调用此方法增加计数
    pub fn increment_rewrite_count(&self) {
        let mut count = self.rewrite_count.lock().unwrap();
        *count += 1;
    }

    /// 获取当前会话的所有记录
    pub fn get_records(&self) -> Vec<DecisionRecord> {
        self.records.lock().unwrap().clone()
    }

    /// 获取当前重写次数
    pub fn get_rewrite_count(&self) -> usize {
        *self.rewrite_count.lock().unwrap()
    }

    /// 导出所有记录为嵌入文本（供长期记忆索引）
    pub fn export_for_embedding(&self) -> Vec<String> {
        self.records
            .lock()
            .unwrap()
            .iter()
            .map(DecisionRecord::to_embedding_text)
            .collect()
    }

    /// 重置会话记录
    pub fn reset(&self) {
        self.records.lock().unwrap().clear();
        *self.rewrite_count.lock().unwrap() = 0;
    }
}

impl Default for DecisionHistoryLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_and_retrieve() {
        let logger = DecisionHistoryLogger::new();

        logger.log_tool_call(
            "search_in_files",
            r#"{"path":"src","pattern":"fn main"}"#,
            true,
            150,
        );
        logger.increment_rewrite_count();
        logger.log_tool_call("read_file", r#"{"path":"src/main.rs"}"#, true, 50);

        let records = logger.get_records();
        assert_eq!(records.len(), 2);
        assert_eq!(logger.get_rewrite_count(), 1);
        assert_eq!(records[0].tool_name, "search_in_files");
        assert!(records[0].success);
    }

    #[test]
    fn test_embedding_export() {
        let logger = DecisionHistoryLogger::new();
        logger.log_tool_call("grep", "pattern", false, 100);

        let embeddings = logger.export_for_embedding();
        assert_eq!(embeddings.len(), 1);
        assert!(embeddings[0].contains("grep"));
    }
}

//! `ManagerAgent` 的轻量单元测试（依赖 `agent_core` 等子模块）。

use super::types::{ManagerAgent, ManagerConfig};

#[tokio::test]
async fn test_decompose_fallback() {
    let manager = ManagerAgent::new(ManagerConfig::default());
    let output = manager.decompose_fallback("测试任务");
    assert_eq!(output.sub_goals.len(), 1);
}

//! **E（Execute）** 步：工具批执行。实现位于 **`tools`** 子模块；`agent_turn` 根再导出为 **`execute_tools`** 以保持既有路径。

pub(crate) mod tools;

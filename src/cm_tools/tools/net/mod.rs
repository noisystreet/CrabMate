//! 网络类工具：`web_search`（联网搜索）与 `weather`（天气查询）。
//!
//! `#1530` Step 2：两者原先在 `tools/` 顶层平铺，按域归位后由本模块统一声明。

pub(crate) mod weather;
pub(crate) mod web_search;

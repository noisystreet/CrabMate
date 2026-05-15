//! DSML 字面量常量（独立文件，避免部分静态分析器把 `</…` 误解析进后续函数体）。

pub(super) const DSML_OPEN_FW: &str = "<｜DSML｜";
pub(super) const DSML_CLOSE_FW: &str = "</｜DSML｜";
pub(super) const DSML_OPEN_ASCII: &str = "<|DSML|";
pub(super) const DSML_CLOSE_ASCII: &str = "</|DSML|";

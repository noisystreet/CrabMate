//! 完成判定的**结构化证据采集**（原第 ① 层）。
//!
//! 原第 ① 层的「领域检测器 + 通用兜底」已整层删除（#1415）：检测器只有 C++ 一个、本身也是
//! 文本启发式，保留它意味着同一事实存在两条判定路径。现在本模块只采集结构化事实，
//! 供第 ② 层语义判定消费，**不自下完成结论**。

mod evidence;

pub use evidence::{EvidenceItem, EvidenceKind, EvidenceSet, EvidenceStrength};
pub(crate) use evidence::command_line_looks_like_verify;

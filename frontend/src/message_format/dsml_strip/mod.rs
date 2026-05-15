//! 与后端 `text_sanitize::strip_deepseek_dsml_for_display` 对齐的展示剥离（WASM 无 `regex` 依赖）。
//!
//! 用于 Web 气泡在 **`assistant_text_for_display`** 路径上剥掉 DeepSeek DSML 噪声，避免与 CLI/TUI 已剥内容不一致。

mod named;
mod normalize;
mod orphans;
mod tags;
mod tagged;

use named::{strip_dsml_named_blocks_ascii, strip_dsml_named_blocks_fullwidth};
use normalize::{collapse_blank_runs, normalize_deepseek_dsml_vendor_variants};
use orphans::{
    strip_orphan_close_ascii, strip_orphan_close_fw, strip_orphan_open_ascii, strip_orphan_open_fw,
};
use tagged::strip_tagged_blocks_both_widths;

pub(crate) fn strip_deepseek_dsml_for_display(s: &str) -> String {
    let mut out = normalize_deepseek_dsml_vendor_variants(s);
    if !out.contains("DSML") {
        return out;
    }
    for tag in ["tool_calls", "parameter", "invoke", "function_calls"] {
        out = strip_tagged_blocks_both_widths(out, tag);
    }
    out = strip_dsml_named_blocks_fullwidth(&out);
    out = strip_dsml_named_blocks_ascii(&out);
    out = strip_orphan_open_fw(&out);
    out = strip_orphan_close_fw(&out);
    out = strip_orphan_open_ascii(&out);
    out = strip_orphan_close_ascii(&out);
    collapse_blank_runs(&out)
}

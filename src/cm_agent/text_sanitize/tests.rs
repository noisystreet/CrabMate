use super::{
    dedupe_plain_assistant_preamble, naturalize_assistant_plan_prose_tail,
    naturalize_plan_step_description,
};

#[test]
fn naturalize_step_extracts_json_description() {
    let raw = r#"{"id":"a","description":"读取配置并汇总"}"#;
    assert_eq!(naturalize_plan_step_description(raw), "读取配置并汇总");
}
#[test]
fn naturalize_step_flattens_markdown_list() {
    let s = "- 查日志\n- 修配置";
    assert_eq!(naturalize_plan_step_description(s), "查日志；修配置");
}

#[test]
fn naturalize_plan_prose_dedupes_adjacent_identical_lines() {
    let line = "我将帮您编写 Hello World，并先规划任务步骤：";
    let raw = format!("{line}\n{line}");
    assert_eq!(naturalize_assistant_plan_prose_tail(&raw), line);
}
#[test]
fn naturalize_plan_prose_dedupes_fullwidth_colon_variant() {
    let a = "我将帮您编写步骤：";
    let b = "我将帮您编写步骤:"; // ASCII colon
    let raw = format!("{a}\n{b}");
    assert_eq!(naturalize_assistant_plan_prose_tail(&raw), a);
}
#[test]
fn naturalize_plan_prose_dedupes_terminal_punctuation_variant() {
    let a = "我将先拆解任务步骤：";
    let b = "我将先拆解任务步骤。";
    let raw = format!("{a}\n{b}");
    assert_eq!(naturalize_assistant_plan_prose_tail(&raw), a);
}
#[test]
fn dedupe_plain_preamble_collapses_space_joined_duplicate() {
    let line = "我将帮您编写 Hello World 并规划步骤。";
    let raw = format!("{line} {line}");
    assert_eq!(dedupe_plain_assistant_preamble(&raw), line);
}

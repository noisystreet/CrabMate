use super::*;
use serde_json::json;

#[test]
fn path_alias_and_case_variant_both_land_on_path() {
    let mut v = json!({ "FilePath": " a.txt " });
    assert!(coerce_args_value(&PATH_ONLY, &mut v));
    assert_eq!(v["path"], "a.txt");
}

#[test]
fn existing_path_wins_over_alias() {
    let mut v = json!({ "path": "keep.txt", "file_path": "drop.txt" });
    coerce_args_value(&PATH_ONLY, &mut v);
    assert_eq!(v["path"], "keep.txt");
}

#[test]
fn u64_field_rejects_fraction_and_below_min() {
    let mut v = json!({ "start_line": 1.5 });
    coerce_args_value(&READ_FILE_POLICY, &mut v);
    assert_eq!(v["start_line"], json!(1.5), "非整数不应被改写成整数");

    let mut v = json!({ "start_line": "0" });
    coerce_args_value(&READ_FILE_POLICY, &mut v);
    assert_eq!(v["start_line"], json!("0"), "低于下界不应被接受");
}

#[test]
fn bool_field_accepts_words_and_digits() {
    let mut v = json!({ "count_total_lines": " YES " });
    assert!(coerce_args_value(&READ_FILE_POLICY, &mut v));
    assert_eq!(v["count_total_lines"], json!(true));
}

#[test]
fn modify_file_top_level_mode_synonyms_include_overwrite_to_full() {
    let mut v = json!({ "path": "a.txt", "mode": "Overwrite" });
    assert!(coerce_args_value(&MODIFY_FILE_POLICY, &mut v));
    assert_eq!(v["mode"], "full");
}

#[test]
fn modify_file_edits_mode_is_restricted_to_line_modes() {
    let mut v = json!({
        "path": "a.txt",
        "edits": [{ "mode": "overwrite", "content": "x" }],
    });
    coerce_args_value(&MODIFY_FILE_POLICY, &mut v);
    assert_eq!(v["edits"][0]["mode"], "overwrite", "edits 不收整覆盖同义词");
}

#[test]
fn create_file_defaults_missing_content() {
    let mut v = json!({ "output_path": "x.txt" });
    assert!(coerce_args_value(&CREATE_FILE_POLICY, &mut v));
    assert_eq!(v["path"], "x.txt");
    assert_eq!(v["content"], "");
}

#[test]
fn from_to_aliases_are_exact_key_match() {
    let mut v = json!({ "src": "a.txt", "dst": "b.txt" });
    assert!(coerce_args_value(&FROM_TO_POLICY, &mut v));
    assert_eq!(v["from"], "a.txt");
    assert_eq!(v["to"], "b.txt");

    let mut v = json!({ "Src": "a.txt", "Dst": "b.txt" });
    assert!(!coerce_args_value(&FROM_TO_POLICY, &mut v));
    assert_eq!(v["Src"], "a.txt", "大小写变体不走 from/to 别名（旧行为）");
}

#[test]
fn no_coercion_policy_is_a_noop() {
    let mut v = json!({ "file_path": "a.txt" });
    assert!(!coerce_args_value(&NO_COERCION, &mut v));
    assert_eq!(v["file_path"], "a.txt");
}

// ── 边角：钉住旧实现（按工具名分派时代）的具体行为 ────────────────────

/// `path: null` 保持原样（旧 `coerce_path_string_field` 对 null 返回 `false`），
/// 交给 schema 报「类型不符」，而不是被悄悄补成字符串。
#[test]
fn null_path_is_left_for_schema_validation() {
    let mut v = json!({ "path": null });
    assert!(!coerce_args_value(&PATH_ONLY, &mut v));
    assert_eq!(v["path"], Value::Null);
}

/// `edits` 不是数组时忽略（旧实现 `if let Some(Value::Array(edits))`）。
#[test]
fn non_array_edits_is_ignored() {
    let mut v = json!({ "path": "a.txt", "edits": { "mode": "lines" } });
    coerce_args_value(&MODIFY_FILE_POLICY, &mut v);
    assert_eq!(v["edits"]["mode"], "lines", "非数组不该被逐项归一");
}

/// `guard` 里已显式给的子键优先，旧平铺键上抬不覆盖它（旧 `or_insert` 语义）。
#[test]
fn guard_explicit_value_wins_over_legacy_flat_key() {
    let mut v = json!({
        "path": "a.txt",
        "guard": { "content": "KEEP" },
        "expect_content": "OLD",
        "expect_first_line": "L1",
    });
    assert!(coerce_args_value(&MODIFY_FILE_POLICY, &mut v));
    assert_eq!(v["guard"]["content"], "KEEP");
    assert_eq!(v["guard"]["first_line"], "L1");
    assert!(v.get("expect_content").is_none());
}

/// `guard` 存在但不是对象：旧实现整个替换为空对象再接收上抬键。
#[test]
fn non_object_guard_is_replaced_before_uplift() {
    let mut v = json!({ "path": "a.txt", "guard": "x", "expect_content": "L1" });
    assert!(coerce_args_value(&MODIFY_FILE_POLICY, &mut v));
    assert_eq!(v["guard"], json!({ "content": "L1" }));
}

/// 顶层 `mode` 的未知值也会被「归一后写回」（旧 `_ => norm`），
/// `edits[i].mode` 则只认表内命中 —— 两条语义必须保持不同。
#[test]
fn unknown_top_level_mode_falls_back_to_normalized_string() {
    let mut v = json!({ "path": "a.txt", "mode": " ReWrite Files " });
    assert!(coerce_args_value(&MODIFY_FILE_POLICY, &mut v));
    assert_eq!(v["mode"], "rewrite_files");
}

/// `after_line` 的下界是 0（`insert_after_line` 允许插到文件开头）。
#[test]
fn after_line_accepts_zero() {
    let mut v = json!({ "path": "a.txt", "after_line": "0" });
    assert!(coerce_args_value(&MODIFY_FILE_POLICY, &mut v));
    assert_eq!(v["after_line"], json!(0));
}

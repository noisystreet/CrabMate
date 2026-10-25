use super::*;

use std::fs;

#[test]
fn lexical_and_broken_relative() {
    let tmp = std::env::temp_dir().join(format!("crabmate_md_links_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs")).unwrap();
    fs::write(
        tmp.join("README.md"),
        "# Hi\n[ok](./docs/a.md)\n[bad](./docs/nope.md)\n",
    )
    .unwrap();
    fs::write(tmp.join("docs/a.md"), "x").unwrap();

    let args = serde_json::json!({ "roots": ["README.md"] }).to_string();
    let out = markdown_check_links_try(&args, &tmp).expect("markdown_check_links should succeed");
    assert!(
        out.contains("目标不存在"),
        "expected missing link report: {}",
        out
    );
    assert!(out.contains("docs/nope.md"), "{}", out);
}

#[test]
fn ref_style_link_resolves() {
    let tmp = std::env::temp_dir().join(format!("crabmate_md_refs_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("d")).unwrap();
    fs::write(tmp.join("x.md"), "[t][r]\n[r]: d/f.md\n").unwrap();
    fs::write(tmp.join("d/f.md"), "ok").unwrap();
    let args = serde_json::json!({ "roots": ["x.md"] }).to_string();
    let out = markdown_check_links_try(&args, &tmp).expect("markdown_check_links should succeed");
    assert!(
        out.contains("未发现已检查的失效链接"),
        "ref link should resolve: {}",
        out
    );
}

#[test]
fn anchor_miss_is_reported() {
    let tmp = std::env::temp_dir().join(format!("crabmate_md_anchor_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs")).unwrap();
    fs::write(
        tmp.join("README.md"),
        "[ok](docs/a.md#hello-world)\n[bad](docs/a.md#missing-anchor)\n",
    )
    .unwrap();
    fs::write(tmp.join("docs/a.md"), "# Hello World\n").unwrap();
    let args = serde_json::json!({ "roots": ["README.md"] }).to_string();
    let out = markdown_check_links_try(&args, &tmp).expect("markdown_check_links should succeed");
    assert!(out.contains("锚点不存在"), "{}", out);
    assert!(out.contains("missing-anchor"), "{}", out);
}

#[test]
fn anchor_check_can_be_disabled() {
    let tmp = std::env::temp_dir().join(format!("crabmate_md_anchor_off_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs")).unwrap();
    fs::write(tmp.join("README.md"), "[bad](docs/a.md#missing-anchor)\n").unwrap();
    fs::write(tmp.join("docs/a.md"), "# Hello World\n").unwrap();
    let args = serde_json::json!({ "roots": ["README.md"], "check_fragments": false }).to_string();
    let out = markdown_check_links_try(&args, &tmp).expect("markdown_check_links should succeed");
    assert!(!out.contains("锚点不存在"), "{}", out);
}

#[test]
fn supports_json_output() {
    let tmp = std::env::temp_dir().join(format!("crabmate_md_json_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    fs::write(tmp.join("README.md"), "[bad](./missing.md)\n").unwrap();
    let args = serde_json::json!({ "roots": ["README.md"], "output_format": "json" }).to_string();
    let out = markdown_check_links_try(&args, &tmp).expect("markdown_check_links should succeed");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json output");
    assert_eq!(v["tool"], "markdown_check_links");
    assert!(v["summary"]["problems"].as_u64().unwrap_or(0) > 0);
}

#[test]
fn supports_sarif_output() {
    let tmp = std::env::temp_dir().join(format!("crabmate_md_sarif_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    fs::write(tmp.join("README.md"), "[bad](./missing.md)\n").unwrap();
    let args = serde_json::json!({ "roots": ["README.md"], "output_format": "sarif" }).to_string();
    let out = markdown_check_links_try(&args, &tmp).expect("markdown_check_links should succeed");
    let v: serde_json::Value = serde_json::from_str(&out).expect("sarif output");
    assert_eq!(v["version"], "2.1.0");
    assert!(v["runs"][0]["results"].is_array());
}

#[test]
fn missing_workspace_root_reports_workspace_failed() {
    // 工作区根 canonicalize 失败是扫描期最早的硬错误，钉住新码与保留的「错误：」前缀。
    let missing = std::env::temp_dir().join(format!(
        "crabmate_md_missing_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&missing);
    let args = serde_json::json!({ "roots": ["README.md"] }).to_string();
    let err = markdown_check_links_try(&args, &missing).expect_err("missing workspace must fail");
    assert_eq!(err.code, "markdown_check_links_workspace_failed");
    assert!(err.message.starts_with("错误："), "{}", err.message);
}

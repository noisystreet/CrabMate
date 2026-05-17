//! `compile_workflow_author_yaml` 金样：`fixtures/workflows/<name>.yaml` ↔ `<name>.expected.json`。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn fixtures_workflows_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/workflows")
}

fn normalize_compile_golden(v: &mut Value) {
    if let Some(obj) = v.as_object_mut() {
        obj.remove("_compile_notes");
    }
}

fn read_yaml_fixture(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "md" || ext == "markdown" {
        return crate::agent::workflow::extract_first_crabmate_workflow_block(&text)
            .unwrap_or_else(|e| panic!("extract markdown {}: {e}", path.display()));
    }
    text
}

#[test]
fn golden_workflow_compile_fixtures_match_expected() {
    let dir = fixtures_workflows_dir();
    let mut cases: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|ext| matches!(ext, "yaml" | "yml" | "md"))
        })
        .collect();
    cases.sort();

    let mut ran = 0usize;
    for yaml_path in cases {
        let stem = yaml_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let expected_path = dir.join(format!("{stem}.expected.json"));
        if !expected_path.is_file() {
            continue;
        }

        let yaml = read_yaml_fixture(&yaml_path);
        let mut got = crate::agent::workflow::compile_workflow_author_yaml(&yaml)
            .unwrap_or_else(|e| panic!("compile {}: {e}", yaml_path.display()));
        normalize_compile_golden(&mut got);

        let expected_text = fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", expected_path.display()));
        let mut want: Value = serde_json::from_str(expected_text.trim())
            .unwrap_or_else(|e| panic!("parse expected {}: {e}", expected_path.display()));
        normalize_compile_golden(&mut want);

        assert_eq!(
            got,
            want,
            "compile golden mismatch for {}",
            yaml_path.display()
        );
        ran += 1;
    }

    assert!(
        ran >= 8,
        "expected at least 8 workflow compile golden cases, ran {ran}"
    );
}

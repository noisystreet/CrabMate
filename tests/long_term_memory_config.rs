//! 长期记忆配置（阶段 1）：`finalize` 对未实现的向量后端组合报错。

use std::io::Write;

#[test]
fn load_config_errors_when_memory_enabled_with_non_disabled_vector_backend() {
    let path = std::env::temp_dir().join(format!(
        "crabmate_ltm_cfg_{}_{}.toml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let mut f = std::fs::File::create(&path).expect("create temp config");
    writeln!(
        f,
        r#"
[agent]
api_base = "https://api.example.com/v1"
model = "deepseek-chat"
system_prompt = "test"
run_command_working_dir = "."
long_term_memory_enabled = true
long_term_memory_vector_backend = "qdrant"
"#
    )
    .expect("write temp config");

    let err = crabmate::load_config(Some(path.to_str().expect("utf8 path")))
        .expect_err("expected config error");
    assert!(
        err.contains("向量后端尚未") || err.contains("long_term_memory"),
        "unexpected message: {err}"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn load_config_accepts_memory_enabled_with_disabled_vector_backend() {
    let path = std::env::temp_dir().join(format!(
        "crabmate_ltm_cfg_ok_{}_{}.toml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let mut f = std::fs::File::create(&path).expect("create temp config");
    writeln!(
        f,
        r#"
[agent]
api_base = "https://api.example.com/v1"
model = "deepseek-chat"
system_prompt = "test"
run_command_working_dir = "."
long_term_memory_enabled = true
long_term_memory_vector_backend = "disabled"
"#
    )
    .expect("write temp config");

    let cfg = crabmate::load_config(Some(path.to_str().expect("utf8 path"))).expect("load ok");
    assert!(cfg.long_term_memory_enabled);
    assert_eq!(cfg.long_term_memory_vector_backend.as_str(), "disabled");
    let _ = std::fs::remove_file(&path);
}

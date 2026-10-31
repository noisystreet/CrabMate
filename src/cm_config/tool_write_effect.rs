//! 「有副作用」的内置工具名单 —— 全仓唯一来源。
//!
//! 这份名单原来有两个各自维护的副本：
//!
//! - `cm_tools::registry_policy`：只读判定（并行只读批、只读 TTL / 同轮去重、L1 只读正列表）；
//! - `cm_memory::memory::codebase_semantic_invalidation`：写工作区之后让语义索引失效。
//!
//! 两份长期漂移：`cm_memory` 那份还停在不存在的 `apply_diff` / `write_file` / `mkdir` 上，
//! 同时漏掉 `structured` / `reminder` / `event` / `archive_*`；`cm_tools` 那份漏掉
//! `cargo_test` / `cargo_run` / `pytest_run`（三者都会执行用户代码）与 `summarize_experience`
//! （写长期记忆），又留着早已不存在的 `run_executable`。名单下沉到本层（`cm_tools` 与
//! `cm_memory` 都已依赖 `cm_config`），两处引用同一份，「只改了其中一处」不再是可能。
//!
//! 口径 —— 这里的「副作用」= 改动工作区文件、执行任意代码、或改变被观察状态：
//!
//! - 写文件：`create_file` / `modify_file` / `apply_patch` / `structured`（`action=patch`）等；
//! - 跑构建脚本或用户代码：`run_command` / `terminal_session` / `cargo_test` / `cargo_run` /
//!   `pytest_run` / `python_snippet_run`；
//! - 改远端或发请求：`http_request` / `gh_*`；
//! - 写持久状态：`long_term_remember` / `long_term_forget` / `summarize_experience` /
//!   `reminder` / `event` / `skill_manage` / `background_job_cancel`。
//!
//! 刻意**不**列入：`cargo_check` / `cargo_clippy`（只读检查；仍会写 `target/` 增量产物，但那是
//! 构建缓存、不改源），以及 `archive_list` / `git` / `code_nav` / `docs_health_sweep` 等纯读工具。
//! `structured` 因把三个只读 action 与写侧 `patch` 合并在同一工具名下而整体算写；同理
//! `reminder` / `event` 的 `list` 也只能跟着算写 —— 按工具名判定的结构不支持 action 级粒度。
//!
//! 新增工具时的自检：`write_effect_list_matches_registered_tools`（`cm_tools` 测试）会同时
//! 抓「名单里有、工具表里没有」的死项与「会写却漏列」的漏项。
//!
//! 名单里的名字全部取自 `crate::cm_names`（内建工具名的单一真源），本文件不写字面量。
//!
//! **为什么常量层独立成顶层模块，而不是放在本层或 `cm_tools`**：本层（`cm_config`）与
//! `cm_tools` 互为上下游 —— `cm_tools` 依赖 `cm_config`，而本文件需要工具名常量。常量若放在
//! `cm_tools`，本层反向引用即构成循环依赖；常量若放在本层，则 `cm_tools` 侧（`specs` /
//! `dev_tag` / `tool_dispatch`）要跟着搬，且工具名是**标识符**而非配置项。`cm_names` 零依赖，
//! 两层都能单向引用，于是这条边不再有方向问题。

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::cm_names as tool_names;

/// 内置工具里带副作用的工具名集合。
///
/// 调用方**不应**依赖其顺序；返回 `&'static` 便于多处零成本共享。
#[must_use]
pub fn builtin_write_effect_tools() -> &'static HashSet<String> {
    static W: OnceLock<HashSet<String>> = OnceLock::new();
    W.get_or_init(|| {
        [
            tool_names::CREATE_FILE,
            tool_names::MODIFY_FILE,
            tool_names::COPY_FILE,
            tool_names::MOVE_FILE,
            tool_names::DELETE_FILES,
            tool_names::DELETE_DIR,
            tool_names::APPEND_FILE,
            tool_names::CREATE_DIR,
            tool_names::SEARCH_REPLACE,
            tool_names::APPLY_PATCH,
            // 合并后整工具算写（见模块文档）。
            tool_names::STRUCTURED,
            // 解压 / 打包都会往工作区写文件；`archive_list` 只读，不列入。
            // 这两个工具只在 `archive-tools` feature 下注册（`SPECS_ARCHIVE` 受 gate），
            // 名单必须同步 gate —— 否则默认构建里「名单有、注册表无」会被
            // `write_effect_list_matches_registered_tools` 判成死项（它检查的正是二者一致）。
            #[cfg(feature = "archive-tools")]
            tool_names::ARCHIVE_PACK,
            #[cfg(feature = "archive-tools")]
            tool_names::ARCHIVE_UNPACK,
            tool_names::LONG_TERM_REMEMBER,
            tool_names::LONG_TERM_FORGET,
            tool_names::SUMMARIZE_EXPERIENCE,
            tool_names::RUN_COMMAND,
            tool_names::TERMINAL_SESSION,
            tool_names::PYTHON_SNIPPET_RUN,
            tool_names::PYTEST_RUN,
            tool_names::CARGO_TEST,
            tool_names::CARGO_RUN,
            tool_names::WORKFLOW_EXECUTE,
            tool_names::HTTP_REQUEST,
            tool_names::GH_API,
            tool_names::GH_PR,
            tool_names::GH_RUN,
            tool_names::GH_ISSUE_CREATE,
            tool_names::GH_RELEASE_CREATE,
            tool_names::SKILL_MANAGE,
            tool_names::BACKGROUND_JOB_CANCEL,
            // 日程/提醒：除 `list` 外都会写 `.crabmate/*.json`。
            tool_names::REMINDER,
            tool_names::EVENT,
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    })
}

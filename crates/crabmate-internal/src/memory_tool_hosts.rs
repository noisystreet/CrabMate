//! `crabmate-internal` 对 [`crabmate_tools::memory_tool_host`] 的实现。

use std::path::Path;
use std::sync::Arc;

use crabmate_config::AgentConfig;
use crabmate_tools::memory_tool_host::{CodebaseSemanticToolHost, LongTermMemoryToolHost};

use crate::long_term_memory_tools::{
    LongTermMemoryHostInner, LongTermMemoryToolState, long_term_forget, long_term_memory_list,
    long_term_remember, summarize_experience,
};
use crate::memory::codebase_semantic_index::{CodebaseSemanticToolParams, run_tool};

pub struct CodebaseSemanticHost {
    pub params: CodebaseSemanticToolParams,
}

impl CodebaseSemanticHost {
    pub fn from_config(cfg: &AgentConfig) -> Self {
        Self {
            params: CodebaseSemanticToolParams::from_agent_config(cfg),
        }
    }

    pub fn from_params(params: CodebaseSemanticToolParams) -> Self {
        Self { params }
    }
}

impl CodebaseSemanticToolHost for CodebaseSemanticHost {
    fn run_search(&self, args_json: &str, working_dir: &Path, max_output_len: usize) -> String {
        run_tool(args_json, working_dir, &self.params, max_output_len)
    }
}

pub struct LongTermMemoryHost {
    inner: LongTermMemoryHostInner,
}

impl LongTermMemoryHost {
    pub fn new(
        rt: Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>,
        scope_id: String,
    ) -> Self {
        Self {
            inner: LongTermMemoryHostInner { rt, scope_id },
        }
    }
}

impl LongTermMemoryToolHost for LongTermMemoryHost {
    fn dispatch(&self, tool_name: &str, args_json: &str, cfg: &AgentConfig) -> String {
        let st = LongTermMemoryToolState {
            cfg,
            rt: &self.inner.rt,
            scope: self.inner.scope_id.as_str(),
        };
        match tool_name {
            "long_term_remember" => long_term_remember(args_json, &st),
            "long_term_forget" => long_term_forget(args_json, &st),
            "long_term_memory_list" => long_term_memory_list(args_json, &st),
            "summarize_experience" => summarize_experience(args_json, &st),
            _ => format!("错误：未知长期记忆工具 `{tool_name}`"),
        }
    }
}

/// 工具 dispatch 路径在栈上构造记忆宿主，供 [`crabmate_tools::tools::ToolContext`] 借用。
pub struct DispatchMemoryHosts {
    pub codebase: CodebaseSemanticHost,
    pub long_term: Option<LongTermMemoryHost>,
}

impl DispatchMemoryHosts {
    pub fn from_dispatch_inputs(
        cfg: &AgentConfig,
        ltm: Option<Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
        scope_id: Option<&str>,
    ) -> Self {
        let codebase = CodebaseSemanticHost::from_config(cfg);
        let (mem_rt, mem_scope) =
            crate::memory::long_term_memory::tool_context_memory_extras(cfg, ltm, scope_id);
        let long_term = mem_rt
            .zip(mem_scope)
            .map(|(rt, scope)| LongTermMemoryHost::new(rt, scope));
        Self {
            codebase,
            long_term,
        }
    }

    pub fn codebase_ref(&self) -> &dyn CodebaseSemanticToolHost {
        &self.codebase
    }

    pub fn long_term_ref(&self) -> Option<&dyn LongTermMemoryToolHost> {
        self.long_term
            .as_ref()
            .map(|h| h as &dyn LongTermMemoryToolHost)
    }
}

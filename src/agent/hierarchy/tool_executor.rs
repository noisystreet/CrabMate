//! 简化工具执行器
//!
//! 供 Operator 在 ReAct 循环中调用真实工具，支持审批流程

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;

use crate::config::AgentConfig;
use crate::tool_registry::{self, ToolRuntime};
use crate::types::{CommandApprovalDecision, FunctionCall, ToolCall};

use std::sync::atomic::{AtomicU64, Ordering};

static TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(1);

/// 工具执行器上下文
pub struct ToolExecutorContext {
    pub cfg: Arc<AgentConfig>,
    pub working_dir: std::path::PathBuf,
    /// 可选的 Web 审批运行时（用于触发审批对话框）
    pub web_tool_runtime: Option<crate::tool_registry::WebToolRuntime>,
}

impl ToolExecutorContext {
    pub fn new(cfg: Arc<AgentConfig>, working_dir: std::path::PathBuf) -> Self {
        Self {
            cfg,
            working_dir,
            web_tool_runtime: None,
        }
    }

    /// 启用 Web 审批流程
    pub fn with_web_approval(
        mut self,
        out_tx: tokio::sync::mpsc::Sender<String>,
        approval_rx: tokio::sync::mpsc::Receiver<CommandApprovalDecision>,
    ) -> Self {
        self.web_tool_runtime = Some(crate::tool_registry::WebToolRuntime {
            out_tx,
            approval_rx_shared: Arc::new(TokioMutex::new(approval_rx)),
            approval_request_guard: Arc::new(TokioMutex::new(())),
            persistent_allowlist_shared: Arc::new(TokioMutex::new(HashSet::new())),
        });
        self
    }

    /// 启用 Web 审批流程（使用已包装的 Receiver）
    pub fn with_web_approval_arc(
        mut self,
        out_tx: tokio::sync::mpsc::Sender<String>,
        approval_rx: Arc<TokioMutex<tokio::sync::mpsc::Receiver<CommandApprovalDecision>>>,
    ) -> Self {
        self.web_tool_runtime = Some(crate::tool_registry::WebToolRuntime {
            out_tx,
            approval_rx_shared: approval_rx,
            approval_request_guard: Arc::new(TokioMutex::new(())),
            persistent_allowlist_shared: Arc::new(TokioMutex::new(HashSet::new())),
        });
        self
    }
}

/// 工具执行器
pub struct ToolExecutor {
    ctx: ToolExecutorContext,
}

impl ToolExecutor {
    /// 创建新的工具执行器
    pub fn new(ctx: ToolExecutorContext) -> Self {
        Self { ctx }
    }

    /// 执行单个工具调用（异步，支持审批流程）
    pub async fn execute_tool_call(&self, tool_call: &ToolCall) -> ToolExecutionResult {
        let name = &tool_call.function.name;
        let args = &tool_call.function.arguments;

        log::info!(target: "crabmate", "[HIERARCHICAL] Executing tool: {} with args={}", name, truncate_args(args));

        // 使用 tool_registry::dispatch_tool 以支持审批流程
        let output = self.dispatch_tool_internal(name, args).await;

        // 判断工具执行是否成功
        let success = Self::check_execution_success(name, &output);

        log::info!(target: "crabmate", "[HIERARCHICAL] Tool {} completed, success={}, output_len={}", name, success, output.len());

        // 从输出中提取产物
        let extracted_artifacts =
            extract_artifacts_from_output(name, &output, &self.ctx.working_dir);
        if !extracted_artifacts.is_empty() {
            log::info!(
                target: "crabmate",
                "[HIERARCHICAL] Extracted {} artifacts from tool output",
                extracted_artifacts.len()
            );
        }

        ToolExecutionResult {
            tool_name: name.clone(),
            output: output.clone(),
            error: if success { None } else { Some(output) },
            success,
            extracted_artifacts,
        }
    }

    async fn dispatch_tool_internal(&self, name: &str, args: &str) -> String {
        let mut workspace_changed = false;

        // 构建 ToolRuntime
        let runtime = if let Some(ref web_rt) = self.ctx.web_tool_runtime {
            ToolRuntime::Web {
                workspace_changed: &mut workspace_changed,
                ctx: Some(web_rt),
            }
        } else {
            // 没有审批上下文时，使用一个空的 Web 运行时
            // 这会导致不在白名单的命令返回错误
            ToolRuntime::Web {
                workspace_changed: &mut workspace_changed,
                ctx: None,
            }
        };

        let tc = ToolCall {
            id: format!(
                "hierarchical_{}",
                TOOL_CALL_COUNTER.fetch_add(1, Ordering::Relaxed)
            ),
            typ: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: args.to_string(),
            },
        };

        let (output, _) = tool_registry::dispatch_tool(tool_registry::DispatchToolParams {
            runtime,
            cfg: &self.ctx.cfg,
            effective_working_dir: &self.ctx.working_dir,
            workspace_is_set: true,
            name,
            args,
            tc: &tc,
            read_file_turn_cache: None,
            workspace_changelist: None,
            mcp_session: None,
            turn_allow: None,
            long_term_memory: None,
            long_term_memory_scope_id: None,
        })
        .await;

        output
    }

    /// 检查工具是否存在
    #[allow(dead_code)]
    pub fn has_tool(&self, name: &str) -> bool {
        !name.is_empty()
    }

    /// 检查工具执行是否成功
    ///
    /// 针对不同工具类型使用不同的成功判断逻辑：
    /// - 编译/构建命令：允许警告（warning），只检查致命错误
    /// - 其他命令：检查是否包含错误关键词
    fn check_execution_success(tool_name: &str, output: &str) -> bool {
        // 首先检查是否有明确的失败标记
        let has_explicit_error = output.contains("错误：")
            || output.contains("error:")
            || output.contains("Error:")
            || output.contains("致命错误")
            || output.contains("fatal error");

        // 检查是否是编译/构建相关命令
        let is_build_command = matches!(tool_name, "run_command" | "cmake" | "make")
            || output.contains("make:")
            || output.contains("g++")
            || output.contains("gcc")
            || output.contains("cmake");

        if is_build_command {
            // 对于编译命令，需要更智能的判断：
            // 1. 如果有致命错误，则失败
            if has_explicit_error {
                return false;
            }

            // 2. 检查是否有编译器错误（不是警告）
            // 编译器错误通常包含 "error:" 且不在注释中
            let lines: Vec<&str> = output.lines().collect();
            for line in &lines {
                let line_lower = line.to_lowercase();
                // 真正的编译错误（不是警告）
                if line_lower.contains("error:") &&
                    !line_lower.contains("warning:") &&
                    !line_lower.contains("note:") &&
                    // 排除一些常见的非错误情况
                    !line_lower.contains("0 errors") &&
                    !line_lower.contains("no errors")
                {
                    return false;
                }
            }

            // 3. 检查 make 的错误
            if output.contains("make: ***") && output.contains("停止") {
                return false;
            }

            // 4. 其他情况（包括有警告但无错误）视为成功
            return true;
        }

        // 非编译命令：使用严格的错误检查
        !has_explicit_error
    }
}

/// 工具执行结果
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub tool_name: String,
    pub output: String,
    pub error: Option<String>,
    pub success: bool,
    /// 从输出中提取的产物路径
    pub extracted_artifacts: Vec<ExtractedArtifact>,
}

/// 提取的产物信息
#[derive(Debug, Clone)]
pub struct ExtractedArtifact {
    /// 产物路径
    pub path: std::path::PathBuf,
    /// 产物类型
    pub kind: ExtractedArtifactKind,
    /// 来源工具
    pub source_tool: String,
}

/// 提取的产物类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractedArtifactKind {
    /// 源文件
    SourceFile,
    /// 目标文件
    ObjectFile,
    /// 可执行文件
    Executable,
    /// 静态库
    StaticLibrary,
    /// 动态库
    DynamicLibrary,
    /// 构建目录
    BuildDirectory,
    /// 其他文件
    Other,
}

/// 截断参数用于日志（按字符边界截断，支持中文）
fn truncate_args(args: &str) -> String {
    const MAX_LEN: usize = 100;
    if args.len() > MAX_LEN {
        let truncated = args
            .char_indices()
            .take(MAX_LEN - 3)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &args[..truncated])
    } else {
        args.to_string()
    }
}

/// 从工具输出中提取产物路径
fn extract_artifacts_from_output(
    tool_name: &str,
    output: &str,
    working_dir: &std::path::Path,
) -> Vec<ExtractedArtifact> {
    let mut artifacts = Vec::new();

    // 根据工具类型选择不同的提取策略
    match tool_name {
        "create_file" => {
            // 从 create_file 的输出中提取创建的文件路径
            if let Some(path) = extract_created_file_path(output) {
                let kind = classify_file_by_extension(&path);
                artifacts.push(ExtractedArtifact {
                    path: resolve_path(&path, working_dir),
                    kind,
                    source_tool: tool_name.to_string(),
                });
            }
        }
        "run_command" | "cmake" | "make" => {
            // 从构建命令输出中提取产物
            artifacts.extend(extract_build_artifacts(output, working_dir, tool_name));
        }
        "read_dir" => {
            // 从目录列表中提取文件
            artifacts.extend(extract_files_from_dir_listing(
                output,
                working_dir,
                tool_name,
            ));
        }
        _ => {
            // 通用提取：查找可能的文件路径
            artifacts.extend(extract_generic_files(output, working_dir, tool_name));
        }
    }

    artifacts
}

/// 从 create_file 输出中提取创建的文件路径
fn extract_created_file_path(output: &str) -> Option<String> {
    // 匹配 "已创建文件: path" 或 "Created file: path" 格式
    for line in output.lines() {
        if let Some(idx) = line.find("已创建文件:") {
            let path = line[idx + "已创建文件:".len()..].trim();
            return Some(path.to_string());
        }
        if let Some(idx) = line.find("Created file:") {
            let path = line[idx + "Created file:".len()..].trim();
            return Some(path.to_string());
        }
    }
    None
}

/// 从构建命令输出中提取产物
fn extract_build_artifacts(
    output: &str,
    working_dir: &std::path::Path,
    source_tool: &str,
) -> Vec<ExtractedArtifact> {
    let mut artifacts = Vec::new();

    for line in output.lines() {
        // 匹配 [100%] Linking CXX executable xxx
        if line.contains("Linking")
            && line.contains("executable")
            && let Some(name) = line.split_whitespace().last()
        {
            let path = resolve_path(name, working_dir);
            artifacts.push(ExtractedArtifact {
                path,
                kind: ExtractedArtifactKind::Executable,
                source_tool: source_tool.to_string(),
            });
        }

        // 匹配 Building CXX object xxx.o
        if line.contains("Building")
            && line.contains("object")
            && let Some(name) = line.split_whitespace().last()
        {
            let path = resolve_path(name, working_dir);
            artifacts.push(ExtractedArtifact {
                path,
                kind: ExtractedArtifactKind::ObjectFile,
                source_tool: source_tool.to_string(),
            });
        }

        // 匹配 "-- Configuring done" 后的构建目录
        if (line.contains("Configuring done") || line.contains("Build files have been written to"))
            && let Some(idx) = line.find("to: ")
        {
            let path = &line[idx + 4..].trim();
            artifacts.push(ExtractedArtifact {
                path: resolve_path(path, working_dir),
                kind: ExtractedArtifactKind::BuildDirectory,
                source_tool: source_tool.to_string(),
            });
        }
    }

    artifacts
}

/// 从目录列表中提取文件
fn extract_files_from_dir_listing(
    output: &str,
    working_dir: &std::path::Path,
    source_tool: &str,
) -> Vec<ExtractedArtifact> {
    let mut artifacts = Vec::new();

    for line in output.lines() {
        // 匹配 file: xxx 或 dir: xxx 格式
        if let Some(path) = line.strip_prefix("file: ") {
            let path = path.trim();
            artifacts.push(ExtractedArtifact {
                path: resolve_path(path, working_dir),
                kind: ExtractedArtifactKind::SourceFile,
                source_tool: source_tool.to_string(),
            });
        } else if let Some(path) = line.strip_prefix("dir: ") {
            let path = path.trim();
            artifacts.push(ExtractedArtifact {
                path: resolve_path(path, working_dir),
                kind: ExtractedArtifactKind::BuildDirectory,
                source_tool: source_tool.to_string(),
            });
        }
    }

    artifacts
}

/// 通用文件提取：从输出中查找可能的路径
fn extract_generic_files(
    output: &str,
    working_dir: &std::path::Path,
    source_tool: &str,
) -> Vec<ExtractedArtifact> {
    let mut artifacts = Vec::new();

    // 简单的启发式：查找看起来像路径的字符串
    for line in output.lines() {
        // 查找 ./path 或 path/to/file 格式
        for word in line.split_whitespace() {
            if word.starts_with("./") || word.starts_with("../") {
                let path = resolve_path(word, working_dir);
                if path.exists() {
                    let kind = classify_file_by_extension(&path.to_string_lossy());
                    artifacts.push(ExtractedArtifact {
                        path,
                        kind,
                        source_tool: source_tool.to_string(),
                    });
                }
            }
        }
    }

    artifacts
}

/// 根据文件扩展名分类
fn classify_file_by_extension(path: &str) -> ExtractedArtifactKind {
    let path_lower = path.to_lowercase();
    if path_lower.ends_with(".cpp")
        || path_lower.ends_with(".c")
        || path_lower.ends_with(".h")
        || path_lower.ends_with(".hpp")
        || path_lower.ends_with(".rs")
        || path_lower.ends_with(".py")
        || path_lower.ends_with(".js")
        || path_lower.ends_with(".ts")
        || path_lower.ends_with(".java")
        || path_lower.ends_with(".go")
    {
        ExtractedArtifactKind::SourceFile
    } else if path_lower.ends_with(".o") || path_lower.ends_with(".obj") {
        ExtractedArtifactKind::ObjectFile
    } else if path_lower.ends_with(".a") || path_lower.ends_with(".lib") {
        ExtractedArtifactKind::StaticLibrary
    } else if path_lower.ends_with(".so")
        || path_lower.ends_with(".dll")
        || path_lower.ends_with(".dylib")
    {
        ExtractedArtifactKind::DynamicLibrary
    } else if path_lower.ends_with("/") || !path_lower.contains('.') {
        // 目录或没有扩展名的可执行文件
        ExtractedArtifactKind::Executable
    } else {
        ExtractedArtifactKind::Other
    }
}

/// 解析路径（处理相对路径）
fn resolve_path(path: &str, working_dir: &std::path::Path) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir.join(p)
    }
}

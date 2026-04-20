//! 简化工具执行器
//!
//! 供 Operator 在 ReAct 循环中调用真实工具

use crate::config::AgentConfig;
use crate::tools;
use crate::tools::ToolContext;
use crate::types::ToolCall;

/// 工具执行器
pub struct ToolExecutor {
    /// 持有 owned data 以满足 ToolContext 的生命周期
    _ctx: ToolContextOwned,
}

struct ToolContextOwned {
    cfg: AgentConfig,
    allowed_commands: Vec<String>,
    http_fetch_allowed_prefixes: Vec<String>,
    working_dir: std::path::PathBuf,
}

impl ToolExecutor {
    /// 创建新的工具执行器
    #[allow(dead_code)]
    pub fn new(cfg: &AgentConfig, working_dir: std::path::PathBuf) -> Self {
        let allowed_commands = cfg.allowed_commands.to_vec();
        let http_fetch_allowed_prefixes = cfg.http_fetch_allowed_prefixes.to_vec();

        let owned = ToolContextOwned {
            cfg: cfg.clone(),
            allowed_commands,
            http_fetch_allowed_prefixes,
            working_dir: working_dir.clone(),
        };

        // Safety: ToolContext needs 'static lifetime references, so we need to create it carefully
        // For now, we'll use a simpler approach that works for sync tool execution
        Self { _ctx: owned }
    }

    /// 执行单个工具调用
    #[allow(dead_code)]
    pub fn execute_tool_call(&self, tool_call: &ToolCall) -> ToolExecutionResult {
        let name = &tool_call.function.name;
        let args = &tool_call.function.arguments;

        log::info!(target: "crabmate", "Executing tool: {} with args={}", name, truncate_args(args));

        // 直接创建 ToolContext 并调用
        let output = self.run_tool_internal(name, args);

        let success =
            !output.contains("错误") && !output.contains("error:") && !output.contains("Error:");

        log::info!(target: "crabmate", "Tool {} completed, success={}, output_len={}", name, success, output.len());

        // 从输出中提取产物
        let extracted_artifacts =
            extract_artifacts_from_output(name, &output, &self._ctx.working_dir);
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

    fn run_tool_internal(&self, name: &str, args: &str) -> String {
        let ctx = ToolContext {
            cfg: Some(&self._ctx.cfg),
            codebase_semantic: None,
            command_max_output_len: self._ctx.cfg.command_max_output_len,
            weather_timeout_secs: self._ctx.cfg.weather_timeout_secs,
            allowed_commands: &self._ctx.allowed_commands,
            working_dir: &self._ctx.working_dir,
            web_search_timeout_secs: self._ctx.cfg.web_search_timeout_secs,
            web_search_provider: self._ctx.cfg.web_search_provider,
            web_search_api_key: "",
            web_search_max_results: self._ctx.cfg.web_search_max_results,
            http_fetch_allowed_prefixes: &self._ctx.http_fetch_allowed_prefixes,
            http_fetch_timeout_secs: self._ctx.cfg.http_fetch_timeout_secs,
            http_fetch_max_response_bytes: self._ctx.cfg.http_fetch_max_response_bytes,
            command_timeout_secs: self._ctx.cfg.command_timeout_secs,
            read_file_turn_cache: None,
            workspace_changelist: None,
            test_result_cache_enabled: false,
            test_result_cache_max_entries: 0,
            long_term_memory: None,
            long_term_memory_scope_id: None,
        };

        tools::run_tool(name, args, &ctx)
    }

    /// 检查工具是否存在
    #[allow(dead_code)]
    pub fn has_tool(&self, name: &str) -> bool {
        !name.is_empty()
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
        // 尝试提取文件路径（简单启发式）
        let trimmed = line.trim();
        if trimmed.starts_with("./") || trimmed.starts_with("../") || trimmed.starts_with('/') {
            let path = resolve_path(trimmed, working_dir);
            if path.exists() {
                let kind = classify_file_by_path(&path);
                artifacts.push(ExtractedArtifact {
                    path,
                    kind,
                    source_tool: source_tool.to_string(),
                });
            }
        }
    }

    artifacts
}

/// 通用文件提取（从输出中查找可能的文件路径）
fn extract_generic_files(
    output: &str,
    working_dir: &std::path::Path,
    source_tool: &str,
) -> Vec<ExtractedArtifact> {
    let mut artifacts = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 匹配常见的文件路径模式
    for line in output.lines() {
        // 查找看起来像路径的字符串
        for word in line.split_whitespace() {
            let cleaned =
                word.trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == ';');

            // 检查是否是文件路径
            if (cleaned.contains('/') || cleaned.contains("\\"))
                && !cleaned.starts_with("http")
                && !cleaned.starts_with("git@")
            {
                let path = resolve_path(cleaned, working_dir);
                if path.exists() && path.is_file() {
                    let path_str = path.to_string_lossy().to_string();
                    if seen.insert(path_str.clone()) {
                        let kind = classify_file_by_path(&path);
                        artifacts.push(ExtractedArtifact {
                            path,
                            kind,
                            source_tool: source_tool.to_string(),
                        });
                    }
                }
            }
        }
    }

    artifacts
}

/// 根据扩展名分类文件
fn classify_file_by_extension(path: &str) -> ExtractedArtifactKind {
    let path = std::path::Path::new(path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    match ext.as_deref() {
        Some("c") | Some("cpp") | Some("cc") | Some("cxx") | Some("h") | Some("hpp") => {
            ExtractedArtifactKind::SourceFile
        }
        Some("o") | Some("obj") => ExtractedArtifactKind::ObjectFile,
        Some("a") | Some("lib") => ExtractedArtifactKind::StaticLibrary,
        Some("so") | Some("dll") | Some("dylib") => ExtractedArtifactKind::DynamicLibrary,
        Some("exe") => ExtractedArtifactKind::Executable,
        _ => {
            // 检查文件名是否是常见可执行文件名
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && (name == "main" || name == "a.out" || name.ends_with(".exe"))
            {
                return ExtractedArtifactKind::Executable;
            }
            ExtractedArtifactKind::Other
        }
    }
}

/// 根据完整路径分类文件
fn classify_file_by_path(path: &std::path::Path) -> ExtractedArtifactKind {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        match ext.to_lowercase().as_str() {
            "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" => ExtractedArtifactKind::SourceFile,
            "o" | "obj" => ExtractedArtifactKind::ObjectFile,
            "a" | "lib" => ExtractedArtifactKind::StaticLibrary,
            "so" | "dll" | "dylib" => ExtractedArtifactKind::DynamicLibrary,
            "exe" => ExtractedArtifactKind::Executable,
            _ => ExtractedArtifactKind::Other,
        }
    } else {
        // 无扩展名的文件可能是可执行文件
        if path
            .file_name()
            .is_some_and(|n| n == "main" || n == "a.out")
        {
            ExtractedArtifactKind::Executable
        } else {
            ExtractedArtifactKind::Other
        }
    }
}

/// 解析路径（处理相对路径和绝对路径）
fn resolve_path(path: &str, working_dir: &std::path::Path) -> std::path::PathBuf {
    let path = std::path::Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    }
}

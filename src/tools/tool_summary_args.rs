//! Typed tool-argument shapes for **dynamic** chat summaries (`ToolSummaryKind::Dynamic`).
//! Field names match tool JSON schemas; extra keys are ignored (no `deny_unknown_fields`).
//! Parsing uses `serde_json::from_value`; on failure the caller yields no summary (same as
//! missing required fields in the previous `Value::get` style).

use serde::Deserialize;

/// Deserialize from a clone of `v`, then build the summary line.
pub(super) fn summarize_from_value<T>(v: &serde_json::Value) -> Option<String>
where
    T: serde::de::DeserializeOwned + ToolSummaryLine,
{
    let t: T = serde_json::from_value(v.clone()).ok()?;
    t.summary_line()
}

pub(super) trait ToolSummaryLine {
    fn summary_line(self) -> Option<String>;
}

// ── codebase_semantic_search ──────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct CodebaseSemanticSearchSummaryArgs {
    #[serde(default)]
    rebuild_index: bool,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    incremental: Option<bool>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    retrieve_mode: Option<String>,
}

impl ToolSummaryLine for CodebaseSemanticSearchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        if self.rebuild_index {
            let p = self.path.as_deref().unwrap_or(".").trim();
            let p = if p.is_empty() { "." } else { p };
            return Some(match self.incremental {
                Some(false) => format!("semantic index rebuild full ({})", p),
                _ => format!("semantic index rebuild ({})", p),
            });
        }
        let q = self.query.as_deref().unwrap_or("").trim();
        if q.is_empty() {
            return Some("semantic code search".to_string());
        }
        let mut s: String = q.chars().take(48).collect();
        if q.chars().count() > 48 {
            s.push('…');
        }
        let mode = self
            .retrieve_mode
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or("hybrid");
        Some(format!("code search [{}]: {}", mode, s))
    }
}

// ── search_in_files ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct SearchInFilesSummaryArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for SearchInFilesSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let pattern = self.pattern.trim();
        if pattern.is_empty() {
            return None;
        }
        const MAX_PATTERN_CHARS: usize = 40;
        let mut pat: String = pattern.chars().take(MAX_PATTERN_CHARS).collect();
        if pattern.chars().count() > MAX_PATTERN_CHARS {
            pat.push('…');
        }
        const MAX_PATH_CHARS: usize = 28;
        let sub = self
            .path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        Some(match sub {
            Some(p) => {
                let mut ps: String = p.chars().take(MAX_PATH_CHARS).collect();
                if p.chars().count() > MAX_PATH_CHARS {
                    ps.push('…');
                }
                format!("search in files: {} @ {}", pat, ps)
            }
            None => format!("search in files: {}", pat),
        })
    }
}

// ── run_command, run_executable (string args only in summary) ─

#[derive(Debug, Deserialize)]
pub(super) struct RunCommandSummaryArgs {
    command: String,
    #[serde(default)]
    args: Vec<serde_json::Value>,
}

impl ToolSummaryLine for RunCommandSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let cmd = self.command.trim();
        if cmd.is_empty() {
            return None;
        }
        let args = self
            .args
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if args.is_empty() {
            Some(cmd.to_string())
        } else {
            Some(format!("{} {}", cmd, args))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RunExecutableSummaryArgs {
    path: String,
    #[serde(default)]
    args: Vec<serde_json::Value>,
}

impl ToolSummaryLine for RunExecutableSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let args = self
            .args
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if args.is_empty() {
            Some(format!("run executable: {}", path))
        } else {
            Some(format!("run executable: {} {}", path, args))
        }
    }
}

// ── rust_analyzer * ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct RustAnalyzerGotoDefSummaryArgs {
    path: String,
    #[serde(default)]
    line: Option<u64>,
}

impl ToolSummaryLine for RustAnalyzerGotoDefSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!(
            "rust-analyzer goto definition {}:{}",
            path,
            self.line.unwrap_or(0)
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RustAnalyzerFindRefsSummaryArgs {
    path: String,
    #[serde(default)]
    line: Option<u64>,
}

impl ToolSummaryLine for RustAnalyzerFindRefsSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!(
            "rust-analyzer find references {}:{}",
            path,
            self.line.unwrap_or(0)
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RustAnalyzerHoverSummaryArgs {
    path: String,
    #[serde(default)]
    line: Option<u64>,
}

impl ToolSummaryLine for RustAnalyzerHoverSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!(
            "rust-analyzer hover {}:{}",
            path,
            self.line.unwrap_or(0)
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RustAnalyzerDocSymbolSummaryArgs {
    path: String,
}

impl ToolSummaryLine for RustAnalyzerDocSymbolSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("rust-analyzer document symbols {}", path))
    }
}

// ── python / uv / pre-commit / ast-grep ───────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct PythonInstallEditableSummaryArgs {
    #[serde(default)]
    backend: Option<String>,
}

impl ToolSummaryLine for PythonInstallEditableSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let b = self.backend.as_deref().unwrap_or("?").trim();
        let b = if b.is_empty() { "?" } else { b };
        Some(format!("editable Python install ({})", b))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct UvRunSummaryArgs {
    #[serde(default)]
    args: Vec<serde_json::Value>,
}

impl ToolSummaryLine for UvRunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let args = self
            .args
            .iter()
            .filter_map(|x| x.as_str())
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        if args.is_empty() {
            Some("uv run".to_string())
        } else {
            Some(format!("uv run {}", args))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct PythonSnippetRunSummaryArgs {
    #[serde(default)]
    use_uv: bool,
}

impl ToolSummaryLine for PythonSnippetRunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        Some(if self.use_uv {
            "python snippet (uv)".to_string()
        } else {
            "python snippet".to_string()
        })
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ErrorOutputPlaybookSummaryArgs {
    #[serde(default)]
    ecosystem: Option<String>,
}

impl ToolSummaryLine for ErrorOutputPlaybookSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let eco = self
            .ecosystem
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("auto");
        Some(format!("error playbook ({})", eco))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct PreCommitRunSummaryArgs {
    #[serde(default)]
    hook: Option<String>,
}

impl ToolSummaryLine for PreCommitRunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let hook = self.hook.as_deref().unwrap_or("").trim();
        if hook.is_empty() {
            Some("pre-commit run".to_string())
        } else {
            Some(format!("pre-commit run {}", hook))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct AstGrepRunSummaryArgs {
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
}

impl ToolSummaryLine for AstGrepRunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let lang = self.lang.as_deref().unwrap_or("?");
        let p = self.pattern.as_deref().unwrap_or("");
        let short = if p.chars().count() > 48 {
            format!("{}…", p.chars().take(48).collect::<String>())
        } else {
            p.to_string()
        };
        Some(format!("ast-grep [{}] {}", lang, short))
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub(super) struct AstGrepRewriteSummaryArgs {
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default = "default_true")]
    dry_run: bool,
}

impl ToolSummaryLine for AstGrepRewriteSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let lang = self.lang.as_deref().unwrap_or("?");
        let p = self.pattern.as_deref().unwrap_or("");
        let short = if p.chars().count() > 42 {
            format!("{}…", p.chars().take(42).collect::<String>())
        } else {
            p.to_string()
        };
        Some(format!(
            "ast-grep rewrite [{}] {}{}",
            lang,
            short,
            if self.dry_run { " (dry-run)" } else { "" }
        ))
    }
}

// ── git diff * ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct GitDiffSummaryArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for GitDiffSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let mode = self.mode.as_deref().unwrap_or("working");
        let path = self.path.as_deref().unwrap_or("").trim();
        if path.is_empty() {
            Some(format!("git diff ({})", mode))
        } else {
            Some(format!("git diff ({}): {}", mode, path))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitDiffStatSummaryArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for GitDiffStatSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let mode = self.mode.as_deref().unwrap_or("working");
        let path = self.path.as_deref().unwrap_or("").trim();
        if path.is_empty() {
            Some(format!("git diff --stat ({})", mode))
        } else {
            Some(format!("git diff --stat ({}): {}", mode, path))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitDiffNamesSummaryArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for GitDiffNamesSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let mode = self.mode.as_deref().unwrap_or("working");
        let path = self.path.as_deref().unwrap_or("").trim();
        if path.is_empty() {
            Some(format!("git diff --name-only ({})", mode))
        } else {
            Some(format!("git diff --name-only ({}): {}", mode, path))
        }
    }
}

// ── file ops ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct CreateFileSummaryArgs {
    path: String,
}

impl ToolSummaryLine for CreateFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("create file: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ModifyFileSummaryArgs {
    path: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    start_line: Option<u64>,
    #[serde(default)]
    end_line: Option<u64>,
}

impl ToolSummaryLine for ModifyFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let mode = self.mode.as_deref().unwrap_or("full");
        if mode == "replace_lines" {
            Some(format!(
                "replace lines {}-{} in {}",
                self.start_line.unwrap_or(0),
                self.end_line.unwrap_or(0),
                path
            ))
        } else {
            Some(format!("modify file: {}", path))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CopyFileSummaryArgs {
    from: String,
    to: String,
}

impl ToolSummaryLine for CopyFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let from = self.from.trim();
        let to = self.to.trim();
        if from.is_empty() || to.is_empty() {
            return None;
        }
        Some(format!("copy file: {} → {}", from, to))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct MoveFileSummaryArgs {
    from: String,
    to: String,
}

impl ToolSummaryLine for MoveFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let from = self.from.trim();
        let to = self.to.trim();
        if from.is_empty() || to.is_empty() {
            return None;
        }
        Some(format!("move file: {} → {}", from, to))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ReadFileSummaryArgs {
    path: String,
    #[serde(default)]
    start_line: Option<u64>,
    #[serde(default)]
    end_line: Option<u64>,
    #[serde(default)]
    max_lines: Option<u64>,
    #[serde(default)]
    encoding: Option<String>,
}

impl ToolSummaryLine for ReadFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let enc = self
            .encoding
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let suffix = match (self.start_line, self.end_line, self.max_lines) {
            (Some(s), Some(e), _) => format!(" [{}-{}]", s, e),
            (Some(s), None, Some(m)) => format!(" [{}~ max_lines={}]", s, m),
            (Some(s), None, None) => format!(" [{}~]", s),
            (None, Some(e), _) => format!(" [1-{}]", e),
            (None, None, Some(m)) => format!(" [chunk max_lines={}]", m),
            (None, None, None) => String::new(),
        };
        let enc_s = enc.map(|e| format!(" enc={}", e)).unwrap_or_default();
        Some(format!("read file: {}{}{}", path, suffix, enc_s))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ReadDirSummaryArgs {
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for ReadDirSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.as_deref().unwrap_or(".").trim();
        let p = if path.is_empty() { "." } else { path };
        Some(format!("read dir: {}", p))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct WebSearchSummaryArgs {
    query: String,
}

impl ToolSummaryLine for WebSearchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let q = self.query.trim();
        if q.is_empty() {
            return None;
        }
        Some(format!("web search: {}", q))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct HttpFetchSummaryArgs {
    url: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    text_format: Option<String>,
}

impl ToolSummaryLine for HttpFetchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let u = self.url.trim();
        if u.is_empty() {
            return None;
        }
        let m = self
            .method
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("GET");
        let tf = self
            .text_format
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase().replace('-', "_"));
        let suffix = match tf.as_deref() {
            Some("html_text" | "htmltext" | "text") => " [html_text]",
            _ => "",
        };
        Some(format!("HTTP {} {}{}", m.to_ascii_uppercase(), u, suffix))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct HttpRequestSummaryArgs {
    url: String,
    method: String,
    #[serde(default)]
    text_format: Option<String>,
}

impl ToolSummaryLine for HttpRequestSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let u = self.url.trim();
        let m = self.method.trim();
        if u.is_empty() || m.is_empty() {
            return None;
        }
        let tf = self
            .text_format
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase().replace('-', "_"));
        let suffix = match tf.as_deref() {
            Some("html_text" | "htmltext" | "text") => " [html_text]",
            _ => "",
        };
        Some(format!("HTTP {} {}{}", m.to_ascii_uppercase(), u, suffix))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GlobFilesSummaryArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for GlobFilesSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let pat = self.pattern.trim();
        if pat.is_empty() {
            return None;
        }
        let root = self.path.as_deref().unwrap_or(".").trim();
        Some(format!(
            "glob {} @ {}",
            pat,
            if root.is_empty() { "." } else { root }
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct MarkdownCheckLinksSummaryArgs {
    #[serde(default)]
    roots: Vec<String>,
}

impl ToolSummaryLine for MarkdownCheckLinksSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let roots = self
            .roots
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        let roots = if roots.is_empty() {
            "README.md, docs".to_string()
        } else {
            roots
        };
        Some(format!("markdown link check: {}", roots))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct StructuredValidateSummaryArgs {
    path: String,
}

impl ToolSummaryLine for StructuredValidateSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("structured validate: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct StructuredQuerySummaryArgs {
    path: String,
    query: String,
}

impl ToolSummaryLine for StructuredQuerySummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        let q = self.query.trim();
        if path.is_empty() || q.is_empty() {
            return None;
        }
        Some(format!("structured query: {} [{}]", path, q))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct StructuredDiffSummaryArgs {
    path_a: String,
    path_b: String,
}

impl ToolSummaryLine for StructuredDiffSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let a = self.path_a.trim();
        let b = self.path_b.trim();
        if a.is_empty() || b.is_empty() {
            return None;
        }
        Some(format!("structured diff: {} vs {}", a, b))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct StructuredPatchSummaryArgs {
    path: String,
    query: String,
    #[serde(default)]
    action: Option<String>,
}

impl ToolSummaryLine for StructuredPatchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let p = self.path.trim();
        let q = self.query.trim();
        if p.is_empty() || q.is_empty() {
            return None;
        }
        let a = self.action.as_deref().unwrap_or("set");
        Some(format!("structured patch: {} [{} @ {}]", p, a, q))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ListTreeSummaryArgs {
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for ListTreeSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let root = self.path.as_deref().unwrap_or(".").trim();
        Some(format!(
            "list tree: {}",
            if root.is_empty() { "." } else { root }
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct HashFileSummaryArgs {
    path: String,
    #[serde(default)]
    algorithm: Option<String>,
}

impl ToolSummaryLine for HashFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let algo = self.algorithm.as_deref().unwrap_or("sha256");
        Some(format!("file hash {}: {}", algo, path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ExtractInFileSummaryArgs {
    path: String,
    pattern: String,
    #[serde(default)]
    encoding: Option<String>,
}

impl ToolSummaryLine for ExtractInFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        let pattern = self.pattern.trim();
        if path.is_empty() || pattern.is_empty() {
            return None;
        }
        let enc = self
            .encoding
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let enc_s = enc.map(|e| format!(" enc={}", e)).unwrap_or_default();
        Some(format!("extract in file: {} / {}{}", path, pattern, enc_s))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ApplyPatchSummaryArgs {
    patch: String,
}

impl ToolSummaryLine for ApplyPatchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let files = self
            .patch
            .lines()
            .filter_map(|line| line.strip_prefix("+++ "))
            .map(|s| s.split_whitespace().next().unwrap_or(""))
            .filter(|s| !s.is_empty() && *s != "/dev/null")
            .map(|s| {
                s.trim_start_matches("b/")
                    .trim_start_matches("a/")
                    .to_string()
            })
            .collect::<Vec<_>>();
        if files.is_empty() {
            Some("apply patch".to_string())
        } else {
            Some(format!("apply patch: {}", files.join(", ")))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct PackageQuerySummaryArgs {
    package: String,
    #[serde(default)]
    manager: Option<String>,
}

impl ToolSummaryLine for PackageQuerySummaryArgs {
    fn summary_line(self) -> Option<String> {
        let pkg = self.package.trim();
        if pkg.is_empty() {
            return None;
        }
        let mgr = self
            .manager
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("auto");
        Some(format!("package query: {} ({})", pkg, mgr))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct FindSymbolSummaryArgs {
    symbol: String,
}

impl ToolSummaryLine for FindSymbolSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let s = self.symbol.trim();
        if s.is_empty() {
            return None;
        }
        Some(format!("find symbol: {}", s))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct FindReferencesSummaryArgs {
    symbol: String,
}

impl ToolSummaryLine for FindReferencesSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let s = self.symbol.trim();
        if s.is_empty() {
            return None;
        }
        Some(format!("find references: {}", s))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CallGraphSketchSummaryArgs {
    #[serde(default)]
    symbols: Vec<String>,
    #[serde(default)]
    symbol: Option<String>,
}

impl ToolSummaryLine for CallGraphSketchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let mut v: Vec<String> = self
            .symbols
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if let Some(s) = self.symbol {
            let t = s.trim().to_string();
            if !t.is_empty() {
                v.push(t);
            }
        }
        v.sort();
        v.dedup();
        if v.is_empty() {
            return None;
        }
        const MAX: usize = 3;
        let head: Vec<_> = v.iter().take(MAX).cloned().collect();
        let mut out = format!("impact sketch: {}", head.join(", "));
        if v.len() > MAX {
            out.push_str(&format!(" +{}", v.len() - MAX));
        }
        Some(out)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RustFileOutlineSummaryArgs {
    path: String,
}

impl ToolSummaryLine for RustFileOutlineSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("Rust outline: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct FormatCheckFileSummaryArgs {
    path: String,
}

impl ToolSummaryLine for FormatCheckFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("format check: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ConvertUnitsSummaryArgs {
    category: String,
    from: String,
    to: String,
}

impl ToolSummaryLine for ConvertUnitsSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let cat = self.category.trim();
        let from = self.from.trim();
        let to = self.to.trim();
        if cat.is_empty() || from.is_empty() || to.is_empty() {
            return None;
        }
        Some(format!("convert units: {} ({} → {})", cat, from, to))
    }
}

// ── Git write summaries ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct GitCheckoutSummaryArgs {
    target: String,
    #[serde(default)]
    create: bool,
}

impl ToolSummaryLine for GitCheckoutSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let target = self.target.trim();
        if target.is_empty() {
            return None;
        }
        if self.create {
            Some(format!("git checkout -b {}", target))
        } else {
            Some(format!("git checkout {}", target))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitBranchCreateSummaryArgs {
    name: String,
}

impl ToolSummaryLine for GitBranchCreateSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return None;
        }
        Some(format!("git branch create: {}", name))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitBranchDeleteSummaryArgs {
    name: String,
}

impl ToolSummaryLine for GitBranchDeleteSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return None;
        }
        Some(format!("git branch delete: {}", name))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitPushSummaryArgs {
    #[serde(default)]
    remote: Option<String>,
    #[serde(default)]
    branch: Option<String>,
}

impl ToolSummaryLine for GitPushSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let remote = self.remote.as_deref().unwrap_or("origin");
        let branch = self.branch.as_deref().unwrap_or("").trim();
        if branch.is_empty() {
            Some(format!("git push {}", remote))
        } else {
            Some(format!("git push {} {}", remote, branch))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitMergeSummaryArgs {
    branch: String,
}

impl ToolSummaryLine for GitMergeSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let branch = self.branch.trim();
        if branch.is_empty() {
            return None;
        }
        Some(format!("git merge {}", branch))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitRebaseSummaryArgs {
    #[serde(default)]
    abort: bool,
    #[serde(default, rename = "continue")]
    continue_rebase: bool,
    #[serde(default)]
    onto: Option<String>,
}

impl ToolSummaryLine for GitRebaseSummaryArgs {
    fn summary_line(self) -> Option<String> {
        if self.abort {
            return Some("git rebase --abort".to_string());
        }
        if self.continue_rebase {
            return Some("git rebase --continue".to_string());
        }
        let onto = self.onto.as_deref().unwrap_or("?");
        Some(format!("git rebase onto {}", onto))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitStashSummaryArgs {
    #[serde(default)]
    action: Option<String>,
}

impl ToolSummaryLine for GitStashSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let action = self.action.as_deref().unwrap_or("push");
        Some(format!("git stash {}", action))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitTagSummaryArgs {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

impl ToolSummaryLine for GitTagSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let action = self.action.as_deref().unwrap_or("list");
        match action {
            "create" => {
                let name = self.name.as_deref().unwrap_or("?");
                Some(format!("git tag create: {}", name))
            }
            "delete" => {
                let name = self.name.as_deref().unwrap_or("?");
                Some(format!("git tag delete: {}", name))
            }
            _ => Some("git tag list".to_string()),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitResetSummaryArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    target: Option<String>,
}

impl ToolSummaryLine for GitResetSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let mode = self.mode.as_deref().unwrap_or("mixed");
        let target = self.target.as_deref().unwrap_or("HEAD");
        Some(format!("git reset --{} {}", mode, target))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitRevertSummaryArgs {
    #[serde(default)]
    abort: bool,
    #[serde(default)]
    commit: Option<String>,
}

impl ToolSummaryLine for GitRevertSummaryArgs {
    fn summary_line(self) -> Option<String> {
        if self.abort {
            return Some("git revert --abort".to_string());
        }
        let commit = self.commit.as_deref().unwrap_or("?");
        Some(format!("git revert {}", commit))
    }
}

// ── Node.js ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct NpmRunSummaryArgs {
    script: String,
}

impl ToolSummaryLine for NpmRunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let script = self.script.trim();
        if script.is_empty() {
            return None;
        }
        Some(format!("npm run {}", script))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct NpxRunSummaryArgs {
    package: String,
}

impl ToolSummaryLine for NpxRunSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let pkg = self.package.trim();
        if pkg.is_empty() {
            return None;
        }
        Some(format!("npx {}", pkg))
    }
}

// ── Process & ports ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct PortCheckSummaryArgs {
    port: u64,
}

impl ToolSummaryLine for PortCheckSummaryArgs {
    fn summary_line(self) -> Option<String> {
        Some(format!("port check: {}", self.port))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ProcessListSummaryArgs {
    #[serde(default)]
    filter: Option<String>,
}

impl ToolSummaryLine for ProcessListSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let filter = self.filter.as_deref().unwrap_or("").trim();
        if filter.is_empty() {
            Some("list processes".to_string())
        } else {
            Some(format!("list processes (filter: {})", filter))
        }
    }
}

// ── Code metrics ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct CodeStatsSummaryArgs {
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for CodeStatsSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.as_deref().unwrap_or(".").trim();
        Some(format!("code stats: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct DependencyGraphSummaryArgs {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}

impl ToolSummaryLine for DependencyGraphSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let format = self.format.as_deref().unwrap_or("mermaid");
        let kind = self.kind.as_deref().unwrap_or("auto");
        Some(format!("dependency graph ({}/{})", kind, format))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CoverageReportSummaryArgs {
    #[serde(default)]
    path: Option<String>,
}

impl ToolSummaryLine for CoverageReportSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.as_deref().unwrap_or("").trim();
        if path.is_empty() {
            Some("coverage report (auto-detect)".to_string())
        } else {
            Some(format!("coverage report: {}", path))
        }
    }
}

// ── More file tools ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct DeleteDirSummaryArgs {
    path: String,
    #[serde(default)]
    recursive: bool,
}

impl ToolSummaryLine for DeleteDirSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        if self.recursive {
            Some(format!("delete directory (recursive): {}", path))
        } else {
            Some(format!("delete directory: {}", path))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct SearchReplaceSummaryArgs {
    path: String,
    search: String,
    #[serde(default = "default_true")]
    dry_run: bool,
}

impl ToolSummaryLine for SearchReplaceSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let search = self.search.trim();
        if search.is_empty() {
            return None;
        }
        let short = if search.chars().count() > 30 {
            format!("{}…", search.chars().take(30).collect::<String>())
        } else {
            search.to_string()
        };
        Some(format!(
            "search-replace{}: {} / \"{}\"",
            if self.dry_run { " (preview)" } else { "" },
            path,
            short
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ChmodFileSummaryArgs {
    path: String,
    mode: String,
}

impl ToolSummaryLine for ChmodFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        let mode = self.mode.trim();
        if path.is_empty() || mode.is_empty() {
            return None;
        }
        Some(format!("chmod: {} → {}", path, mode))
    }
}

// ── gh * ──────────────────────────────────────────────────────

fn gh_repo_suffix(repo: Option<String>) -> String {
    repo.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|r| format!(" ({})", r))
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrListSummaryArgs {
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    limit: Option<u64>,
}

impl ToolSummaryLine for GhPrListSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let st = self.state.as_deref().unwrap_or("open");
        let lim = self.limit.unwrap_or(30);
        Some(format!(
            "gh pr list{} state={} limit={}",
            gh_repo_suffix(self.repo),
            st,
            lim
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrNumberSummaryArgs {
    #[serde(default)]
    repo: Option<String>,
    number: u64,
}

impl ToolSummaryLine for GhPrNumberSummaryArgs {
    fn summary_line(self) -> Option<String> {
        Some(format!(
            "gh pr view #{}{}",
            self.number,
            gh_repo_suffix(self.repo)
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhIssueListSummaryArgs {
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    limit: Option<u64>,
}

impl ToolSummaryLine for GhIssueListSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let st = self.state.as_deref().unwrap_or("open");
        let lim = self.limit.unwrap_or(30);
        Some(format!(
            "gh issue list{} state={} limit={}",
            gh_repo_suffix(self.repo),
            st,
            lim
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhIssueViewSummaryArgs {
    #[serde(default)]
    repo: Option<String>,
    number: u64,
}

impl ToolSummaryLine for GhIssueViewSummaryArgs {
    fn summary_line(self) -> Option<String> {
        Some(format!(
            "gh issue view #{}{}",
            self.number,
            gh_repo_suffix(self.repo)
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhRunListSummaryArgs {
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    limit: Option<u64>,
}

impl ToolSummaryLine for GhRunListSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let lim = self.limit.unwrap_or(30);
        Some(format!(
            "gh run list{} limit={}",
            gh_repo_suffix(self.repo),
            lim
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhPrDiffSummaryArgs {
    #[serde(default)]
    repo: Option<String>,
    number: u64,
}

impl ToolSummaryLine for GhPrDiffSummaryArgs {
    fn summary_line(self) -> Option<String> {
        Some(format!(
            "gh pr diff #{}{}",
            self.number,
            gh_repo_suffix(self.repo)
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhRunViewSummaryArgs {
    run_id: String,
    #[serde(default)]
    log: bool,
}

impl ToolSummaryLine for GhRunViewSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let id = self.run_id.trim();
        if id.is_empty() {
            return None;
        }
        Some(format!(
            "gh run view {}{}",
            id,
            if self.log { " --log" } else { "" }
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhReleaseListSummaryArgs {
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    limit: Option<u64>,
}

impl ToolSummaryLine for GhReleaseListSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let lim = self.limit.unwrap_or(30);
        Some(format!(
            "gh release list{} limit={}",
            gh_repo_suffix(self.repo),
            lim
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhReleaseViewSummaryArgs {
    #[serde(default)]
    repo: Option<String>,
    tag: String,
}

impl ToolSummaryLine for GhReleaseViewSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let tag = self.tag.trim();
        if tag.is_empty() {
            return None;
        }
        let mut t: String = tag.chars().take(32).collect();
        if tag.chars().count() > 32 {
            t.push('…');
        }
        Some(format!(
            "gh release view {}{}",
            t,
            gh_repo_suffix(self.repo)
        ))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhSearchSummaryArgs {
    scope: String,
    query: String,
}

impl ToolSummaryLine for GhSearchSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let scope = self.scope.trim();
        let q = self.query.trim();
        if scope.is_empty() || q.is_empty() {
            return None;
        }
        let mut qs: String = q.chars().take(40).collect();
        if q.chars().count() > 40 {
            qs.push('…');
        }
        Some(format!("gh search {} {}", scope, qs))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GhApiSummaryArgs {
    path: String,
    #[serde(default)]
    method: Option<String>,
}

impl ToolSummaryLine for GhApiSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let method = self
            .method
            .as_deref()
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        let mut p: String = path.chars().take(40).collect();
        if path.chars().count() > 40 {
            p.push('…');
        }
        Some(format!("gh api {} {}", method, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn git_rebase_continue_json_key_maps() {
        let v = json!({ "continue": true });
        let s = summarize_from_value::<GitRebaseSummaryArgs>(&v).expect("summary");
        assert_eq!(s, "git rebase --continue");
    }

    #[test]
    fn ast_grep_rewrite_dry_run_defaults_true() {
        let v = json!({ "lang": "rust", "pattern": "foo" });
        let s = summarize_from_value::<AstGrepRewriteSummaryArgs>(&v).expect("summary");
        assert!(s.contains("(dry-run)"), "got {s}");
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct FileExistsSummaryArgs {
    path: String,
}

impl ToolSummaryLine for FileExistsSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("file exists: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ReadBinaryMetaSummaryArgs {
    path: String,
}

impl ToolSummaryLine for ReadBinaryMetaSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("binary metadata: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct DeleteFileSummaryArgs {
    path: String,
}

impl ToolSummaryLine for DeleteFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("delete file: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct AppendFileSummaryArgs {
    path: String,
}

impl ToolSummaryLine for AppendFileSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("append to file: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateDirSummaryArgs {
    path: String,
}

impl ToolSummaryLine for CreateDirSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("create directory: {}", path))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct SymlinkInfoSummaryArgs {
    path: String,
}

impl ToolSummaryLine for SymlinkInfoSummaryArgs {
    fn summary_line(self) -> Option<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        Some(format!("symlink info: {}", path))
    }
}

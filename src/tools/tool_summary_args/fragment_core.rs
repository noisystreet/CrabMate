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


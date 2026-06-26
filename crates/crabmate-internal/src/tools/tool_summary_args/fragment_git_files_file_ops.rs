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


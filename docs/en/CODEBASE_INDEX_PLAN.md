**Languages / 语言:** [中文](../CODEBASE_INDEX_PLAN.md) · English (this page)

# Workspace-wide code index and incremental cache — product plan

**Revision (implemented)**: the `codebase_semantic_search` SQLite store now includes a **`crabmate_codebase_files`** catalog (`size` / `mtime_ns` / `content_sha256`). **Workspace-wide** `rebuild_index` is **incremental** by default; a **subtree** `path` still replaces that subtree. Rust chunks add lightweight **symbol hints** in the embed text. See `docs/en/DEVELOPMENT.md` and `docs/en/TOOLS.md`.

Directional plan for a **persistent, incrementally updated** unified index of repository source + metadata to speed browsing and search. Implementation must align with **workspace path safety** (`resolve_for_read`, allowed roots, `..` and symlink policy), **secrets and log redaction**, and **P0 Web authentication** (multi-tenant isolation). See `.cursor/rules/security-sensitive-surface.mdc` and [TODOLIST.md](TODOLIST.md) global P0.

---

## 1. Goals and non-goals

### 1.1 Goals

- **Persistent index**: Survive process restarts; avoid full-tree rescans or re-embedding the same files every turn.
- **Incremental updates**: File-granular (optional directory snapshot) change detection; recompute only affected shards; support large repos.
- **Single entry**: Metadata (inventory, language stats, dependency graph summary, optional git HEAD) and **searchable content** (full-text / vector chunks) under one storage/version story.
- **Faster browsing**: Low-cost directory listing, symbol search, snippet search, semantic neighbors for models and tools.

### 1.2 Non-goals (initial release)

- Replace LSP cross-file type inference / go-to-definition precision.
- Sub-second real-time FS watch parity with IDEs (future enhancement).
- Before **P0 auth**, no cross-user shared global index path on Web (default: **workspace + tenant key** isolation).

---

## 2. Relationship to existing features

| Existing | Relationship |
|----------|--------------|
| `ReadFileTurnCache` | Single-turn ephemeral cache; index is **cross-turn persistent**; writes or `workspace_changed` should invalidate or queue reindex. |
| Long-term memory (SQLite + optional fastembed) | Conversation facts, often session-scoped; code index should use a **separate store** (table or file), not mix `scope_id` semantics with chat memory. |
| `project_profile` | Partial **metadata**; index build can **reuse** scan boundaries / ignore rules or extend with file fingerprint + chunk tables. |
| `repo_overview` / `glob_files` / `read_file` | Once built, prefer **`codebase_search`** (or extended tool) hitting the index, fall back to disk on miss. |

---

## 3. Conceptual architecture

Four layers (same SQLite or multiple files; one external **index runtime** handle):

1. **Manifest** — Normalized workspace root, index format version, build config fingerprint (`max_file_bytes`, language include list, ignore rule hash). Optional: `git HEAD`, build time, background cursor.
2. **File catalog** — `rel_path`, `size`, `mtime_ns`, content hash (optional BLAKE3/SHA256), guessed language/type. Enables incremental skip when hash or `(mtime, size)` unchanged.
3. **Chunks** — Chunking strategy: line windows, AST-aware (future), or fixed token-ish length; store `raw_text` or compressed BLOB, line range. **FTS5** (keyword) and **vectors** (semantic) can share chunk id for hybrid ranking (aligned with long-term memory retrieval direction in TODOLIST).
4. **Retrieval** — Internal API: `hybrid_query(workspace_id, query, top_k, filters)`. Expose as **tool** (function calling) + optional first-turn injection (strict `max_chars`).

---

## 4. Persistence and default location

- **Recommended**: **`.crabmate/codebase_index/`** under workspace (or single `codebase_index.sqlite`), same `.crabmate` convention; paths must pass the same **canonical + root boundary** checks as file tools.
- **Forbidden**: Resolving index files outside workspace unless explicitly configured and still under `workspace_allowed_roots`.
- **Ignore**: Default `.gitignore` + repo `.crabmateignore` (add with docs if new); exclude `.env`, secret patterns, huge binaries, `node_modules`/`target`, etc. (configurable).

---

## 5. Incremental strategy

1. **Cold full build**: First enable or version upgrade—background or explicit command full walk (bounded concurrency/CPU).
2. **Incremental**: Scan catalog; compare `mtime/size` or content hash; changed files re-chunk and update FTS/vectors.
3. **Invalidation**: On writes, deletes, `workspace_changed`, mark dirty or enqueue rescan (aligned with `read_file` cache).
4. **Optional**: `notify`/`watch` debounce (phase 2); **scheduled** or **idle chat** merge scans.
5. **Git**: Optional `HEAD` change narrows changed paths (does not replace mtime—edits outside checkout exist).

---

## 6. Security and compliance (acceptance criteria)

- All index path resolution reuses **`canonical_workspace_root` + `resolve_for_read` semantics**; no new bypass.
- Do **not** log full index content; errors must not leak user source.
- Default exclude **`.env`**, common key filenames, **`.pem`**, etc.; configurable globs.
- Web multi-user: **index and query must bind** session/workspace/tenant keys; ship with P0 auth or CLI-only by default.

---

## 7. Phased milestones

### Phase 0 — Design and scaffolding

- Storage choice (single SQLite vs catalog + sqlite).
- Config draft: `codebase_index_enabled`, `codebase_index_path`, `codebase_index_max_file_bytes`, `codebase_index_exclude_globs`, background build toggle.
- Docs: README / DEVELOPMENT / TOOLS placeholders; fill as phases land.

### Phase 1 — File catalog and metadata (no vectors)

- Persistent file catalog + optional tokei/dependency summary cache.
- CLI/Web: manual or startup **`index rebuild`**; `/status` exposes `codebase_index_*` lag/ready.
- New tool: **`codebase_grep_indexed`** or equivalent (FTS or in-memory inverted), validate incremental correctness.

### Phase 2 — Chunking + FTS hybrid

- Chunk table + FTS5; query API and tool params (path prefix, extension filter, `top_k`).
- Integrate with `run_agent_turn`: optional first-turn injection vs tool-only (prefer latter to reduce noise).

### Phase 3 — Vector embeddings + hybrid scoring

- Reuse **fastembed** (same ONNX stack as long-term memory when enabled); chunk embeddings + cosine retrieval.
- **Hybrid rank**: weighted FTS + vector; log degrade path (embed failure → FTS only).
- Large repos: batch queue, cancel, configurable concurrency.

### Phase 4 — UX and ops

- Web: index status, progress, retry, explicit rebuild (auth-gated).
- Optional **Qdrant/pgvector** (aligned with long-term memory TODOLIST; shared client abstraction).
- Benchmark: build time, query P95, reduction in `read_file` calls.

---

## 8. Testing

- **Unit**: traversal boundaries, ignore rules, incremental detection, symlink behavior if allowed.
- **Integration**: temp workspace fixtures; add/change/delete files → index and query consistency.
- **Security regression**: index path outside workspace must fail; sensitive files absent from results.

---

## 9. Open questions (Phase 0)

- Default **auto background** vs **manual only** (CPU spike expectations).
- Share index format with **MCP / external IDE** (likely no—keep internal SQLite).
- **CLI vs Web** same `.crabmate` index file (default yes; clarify `workspace_id` on multi-root Web).

---

## 10. Doc maintenance

- After each phase: remove or merge TODOLIST items; optional short revision note at top of this file (or rely on Git).
- New tools/config keys: follow `.cursor/rules/todolist-and-documentation.mdc`.

---

*Planning document; exact API and config names follow implementation.*

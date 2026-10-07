**Languages / 语言:** [中文](../未来规划功能.md) · English (this page)

# Future planned capabilities

This document holds **directional** product and deployment boundaries that are **not** tracked as open items in `docs/待办清单.md` / `docs/en/TODOLIST.md`. When something ships, update or remove the matching paragraph here and rely on Git history.

---

## Web identity and accounts (out of scope for the CrabMate process)

**Consensus**: **Do not implement a per-user account system inside CrabMate** (sign-up/login, JWT sessions, `user_id`-scoped conversation stores, etc.). The process keeps the shared-secret model: **`web_api_bearer_token`** / **`CM_WEB_API_BEARER_TOKEN`**; `Authorization: Bearer …` or `X-API-Key …` is compared in constant time to that configured value. Success means “authorized caller”, **not** a specific human. Implementation: **`src/web/chat_handlers/auth.rs`**.

**Recommended deployment**: Place an **API gateway, reverse proxy, or BFF** in front of CrabMate (OAuth2/OIDC, Keycloak, Authentik, vendor API gateways, etc.) for identity, quotas, and audit; then inject credentials toward CrabMate:

1. **Pattern A (common)**: After the gateway validates the end user, attach a **Bearer value identical to this process’s `web_api_bearer_token`** (or have a BFF hold that secret and call CrabMate on behalf of users). CrabMate still sees a **single service-level secret**; tenant/user isolation lives in the gateway and your control plane. Fits “one CrabMate per tenant” or “BFF fills the secret”.
2. **Pattern B**: The gateway terminates TLS and user sessions; traffic to CrabMate uses a **fixed shared secret** plus headers such as **`X-User-Id`**. **Note**: CrabMate **does not** today authorize on trusted headers. Any custom extension must ensure **trusted internal network only** and that clients **cannot bypass** the gateway, or headers become a trivial spoofing vector.

**Upstream LLM keys**: **`API_KEY`** (or Web **`client_llm.api_key`**) is for **`chat/completions`** vendors only; it is **not** the same problem as “who may call CrabMate’s HTTP API”. Per-tenant upstream keys belong in the gateway/BFF if needed.

**Optional future (still not “in-process accounts”)**: A small in-repo step might be “multiple service API keys → tenant id mapping”, which is **not** a full IdP. Per-user conversation persistence should stay in **BFF + dedicated storage** or **multiple instances** rather than duplicating identity inside the agent core.

**See also**: **`docs/design/web_api_integration.md`** (bridging, multi-tenant split vs gateway), **`docs/配置说明.md`** / **`docs/en/CONFIGURATION.md`** (Web API auth and reload limits).

---

## Audience role (side-channel critic)

**Consensus**: Optionally add **tool-less** side `chat/completions` calls that emit **structured** commentary on plan / execute / reflect segments—**without replacing** deterministic checks (**`acceptance` / `step_verifier`**, etc.). Must stay bounded, redacted, and clearly scoped vs existing **`final_plan_semantic_check_*`**.

**Design draft** (anchors, input hygiene, output schema, phased rollout, relationship to **`plan_rewrite`**): **`docs/design/audience_critic_role.md`**.

**Tracking**: Open work is listed under **`docs/en/TODOLIST.md`** → **`agent/`** (“Audience role”).

---

## Built-in Web / desktop IDE mode

**Consensus**: The in-app **Chat / Editor** toggle is a **visual editing surface for the agent workflow** (light edits, change review, alignment with SSE workspace writes)—**not** a second VS Code. Full IDE / multi-language LSP belongs in an external editor or the official extension.

**Roadmap** (baseline, Phases 0–5, split vs extension): **`docs/design/ide_mode_roadmap.md`** (Chinese). Parallel track: **`docs/design/vscode_extension.md`**.

---

## Capability gaps and prioritization (improvement summary)

Compared with mainstream open-source agents (CrewAI, AutoGen, Mastra, LangChain Agents, …), the differences below are **not** tracked in `docs/en/TODOLIST.md`; they are ordered by priority for community reference. Authoritative detail lives in the linked docs.

### P0 — Security (deployment and gateway)

| Improvement | Status |
|-------------|--------|
| Web API shared secret + gateway split | **Documented** (see “Web identity and accounts” above; `doctor` includes serve deployment checks; write-tool audit on by default) |
| Workspace path TOCTOU (`openat2` + `RESOLVE_IN_ROOT` across read/write/delete) | **Shipped** (residual risk in `src/workspace/path.rs`) |

### P1 — Architecture (multi-agent and autonomous agent)

| Improvement | Effort |
|-------------|--------|
| Multi-agent collaboration framework (agent instance abstraction + shared message bus/queue; cf. CrewAI role-based agent + task) | High |
| Proactive autonomous agent mode (goal → self-loop until done; can combine with `workflow_execute` DAG) | High |
| MCP server exposure (stdio shipped as `crabmate mcp serve`; remaining: streamable HTTP egress and transport auth) | Low–med |

### P2 — Experience and observability

| Improvement | Effort |
|-------------|--------|
| Benchmark harness adoption (SWE-bench / GAIA / HumanEval on `crabmate bench`; see **`docs/基准测试规划.md`**) | Med |
| Visual workflow editing (DAG node editor reusing `workflow_execute` schema) | High |
| Multi-level self-repair / replanning (per-tool retry → per-step rollback → plan rewrite, on top of `plan_rewrite`) | Med |
| Token / cost estimation (token side partly shipped; remaining: upstream `usage` metadata and cost estimation) | Low |

### P3 — Ecosystem and scalability (mid/long term)

| Improvement | Effort |
|-------------|--------|
| External vector stores (Qdrant / pgvector adapter; decouple `long_term_memory_store` behind a trait; tenant keys track gateway identity, see above) | Med |
| External observability platforms (OpenTelemetry trace export sharing the Chrome Trace span model) | Low |
| Cloud scale-out (Redis/SQS instead of in-process mpsc; shared session storage) | High |

### Sequencing

- **Near term**: Web auth and deployment boundary (production `web_api_require_bearer=true` + non-empty secret; identity/quotas at the gateway) + benchmark baseline (`crabmate bench --benchmark swe_bench`, recorded in **`docs/BENCHMARK_RESULTS.md`**).
- **Mid term**: MCP streamable HTTP egress (aligned with the Web bearer policy) + multi-agent design (start with in-process logical role separation, then independent instances).
- **Long term**: visual orchestration UI + cloud scale-out (frontend engineering and distributed rework, driven by user growth).

### Capability comparison (current state)

| Capability | Mainstream | CrabMate today |
|------------|------------|----------------|
| Multi-agent collaboration | CrewAI, AutoGen | Single agent only (`logical_dual_agent` is logical role separation, not independent collaboration) |
| Visual orchestration | Mastra, Flowise, LangFlow | No UI orchestrator |
| Benchmark evaluation | SWE-bench, GAIA, HumanEval | Harness ready, not adopted |
| External vector stores | Qdrant, pgvector, Pinecone | Local fastembed only |
| Bidirectional MCP | MCP ecosystem | client side + server stdio (`mcp serve`) work; streamable HTTP egress pending |
| Token / cost estimation | LangChain Usage Tracking | token budget / context chip shipped; cost estimation not implemented |
| Proactive autonomous agent | AutoGPT, BabyAGI | Reactive; tools called on demand |
| Multi-level self-repair | AutoGPT retries, LangChain Plan-and-Execute | Basic plan rewrite (`plan_rewrite`), needs hardening |
| Cloud scale-out | Mastra, CrewAI cloud | Single process; distribution pending |
| External observability | LangSmith, OpenTelemetry | Chrome Trace only |
| API auth (per-user accounts) | — | Out of process (gateway / BFF); see above |

---

*Maintenance: user-visible deployment and security changes still belong in `README.md` / configuration docs; this file is planning narrative, not the source of truth for flags.*

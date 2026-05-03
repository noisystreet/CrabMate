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

**Tracking**: Open work is listed under **`docs/en/TODOLIST.md`** → **`agent/`** (“Audience role”); P-E-V alignment is described in **`docs/规划执行验证架构.md`** / **`docs/en/PLAN_EXECUTE_VERIFY_ARCHITECTURE.md`** §2.3.

---

*Maintenance: user-visible deployment and security changes still belong in `README.md` / configuration docs; this file is planning narrative, not the source of truth for flags.*

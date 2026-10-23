# `.cursor/rules/` — IDE-only long form

**Agents read the repository-root [`AGENTS.md`](../../AGENTS.md), not these files.**

Since `.cursor/rules` compatibility was removed from the server (see `CHANGELOG.md`, the `agents_md_*` change), the only rule source injected into a running agent's `system` prompt is the workspace-root `AGENTS.md`. The `*.mdc` files here are kept for the **Cursor / Trae IDE** integration and as long-form rationale.

## Relationship

| Location | Read by | Role |
|----------|---------|------|
| `AGENTS.md` → **Repository engineering rules** | the agent (injected into `system`) | Authoritative, compressed, enforceable |
| `.cursor/rules/*.mdc` | Cursor / Trae IDE only | Long-form explanation, examples, checklists |
| `.trae/rules/*.mdc` | Trae IDE only | Same content as `.cursor/rules/` (kept in sync manually) |

## Keeping them in sync

When you change a rule:

1. Edit `AGENTS.md` → **Repository engineering rules** (the agent-facing requirement).
2. Mirror the detail into the matching `.mdc` here if the change has nuance worth spelling out.
3. If `.trae/rules/` carries a copy of that file, update it too.

Each `AGENTS.md` subsection names the `.mdc` it mirrors, so a search for the `.mdc` filename finds both halves.

## Why not delete these files

They serve IDE users who are not running the agent, and they hold examples that would bloat the always-injected `system` prompt. If the IDE integration is dropped, delete them together with the references in `.github/pull_request_template.md`, `.github/SECURITY.md`, `.pre-commit-config.yaml`, and `docs/design/`.

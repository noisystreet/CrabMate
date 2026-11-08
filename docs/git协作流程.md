# Git 协作流程（GitHub Flow · 分支 · 提交 · Stacked PR）

> 本文件是 `AGENTS.md` →「Repository engineering rules」中 git / GitHub 协作条款的**细则**。
> `AGENTS.md` 只保留硬红线摘要；本文件放完整流程与排障步骤。
> 对外可见写操作的授权边界（关 Issue、merge、push、Release）见 `AGENTS.md` →「Outbound writes」。
>
> 以下正文沿用原 `AGENTS.md` 的英文原文，未做改写，以免语义漂移。

## 1. GitHub Flow

- Use **GitHub Flow** as the default delivery model: branch from `main` for each change, open a PR, and merge back to
  `main` after checks pass and review is done.
- Keep `main` always releasable: avoid direct pushes to `main` for feature work; prefer short-lived branches and small PRs.
- **Start from an Issue**: new plans, designs, and non-trivial refactors must have a tracking Issue first
  (`gh issue create`). The body states the problem, the intended scope, and the explicit non-goals, and cites the
  relevant `docs/` anchor. Trivial fixes (typo, one-line bug fix) may skip the Issue; a design draft stays in
  `agent_space/` until the Issue settles it, then graduates into `docs/design/`.
- **Link the PR to its Issue**: before merging, the PR body references the tracking Issue — `Closes #123` when the PR
  completes it, `Refs #123` when it only advances it (keep the Issue open in that case). Do not silently widen scope
  inside a PR; file a follow-up Issue instead.
- **Branch naming**: `type/short-slug`, where `type` is the commit type (`feat` / `fix` / `docs` / `refactor` / `test` /
  `perf` / `build` / `ci` / `chore`). Include the Issue number when one exists (`feat/842-ltm-hybrid-retrieval`).
  `main` is the only long-lived branch; delete the branch after merge.
- Before opening or merging PRs, run required local checks (at least `pre-commit run --all-files`) and ensure
  GitHub Actions CI is green.
- Commit messages must follow repository rules (Conventional Commits + bilingual subject); do not bypass hooks unless
  explicitly approved.
- **`--no-verify` is strictly prohibited.** Never use `git commit --no-verify` or `git push --no-verify` to bypass
  pre-commit hooks. All commits must pass the full pre-commit check suite. If hooks are slow, use
  `git commit -m "message"` without `--no-verify` and let hooks run to completion. This rule applies to all code
  generation and automation tools in this workspace.
- Prefer **rebase on latest `origin/main`** before push/merge to minimize drift and conflict risk.

## 2. Stacked PRs（base 在另一个分支上）

When a PR depends on an unmerged PR, point its base at that branch and say so in the body — the downstream PR then
  needs only a re-alignment once, not a second review.

**Expect every downstream PR to turn `CONFLICTING` when its base lands.** That is bookkeeping, not a content conflict:
  this repo merges **rebase-only** (`allow_squash_merge` / `allow_merge_commit` are off), so landed commits get **new
  SHAs**; the downstream branch still carries the old ones, its merge-base drifts, and `delete_branch_on_merge`
  retargets the PR to `main`. The trees may be identical while the PR reports conflicts.

Re-align by rebasing (not by merging `main` into the branch):

```bash
git switch <downstream-branch>
git rebase origin/main           # git drops the commits already upstream: "skipped previously applied commit"
git diff --stat <old-head> HEAD  # empty ⇒ content unchanged, so the earlier local test results still hold
git ls-remote origin <branch>    # confirm the remote head *before* overwriting it
git push --force-with-lease=<branch>:<sha-from-ls-remote> origin <branch>
```

- Use the **explicit** `--force-with-lease=<ref>:<expected-sha>` form. The bare form compares against the local
  remote-tracking ref, which is stale right after a rebase (fails as `stale info`), and it cannot tell "someone else
  pushed" apart from "the ref is gone upstream" — the latter is exactly what happens when the base merged and
  `delete_branch_on_merge` removed it.
- `--force-with-lease` still needs explicit user authorization (see `AGENTS.md` →「Outbound writes」): inspect, then push.
- Merging `origin/main` into the branch clears the `CONFLICTING` banner and needs no force-push — **but in this
  rebase-only repo that is not enough to merge**：rebase-merge 重放的是**每个提交**、并丢掉 merge commit，
  所以「只写在 merge commit 里」的冲突解决对它不可见，GitHub 会继续报
  *"This branch cannot be rebased due to conflicts"*，而 `gh pr view` 同时说 `mergeable: MERGEABLE`。
  先用 `git merge-tree --write-tree --merge-base=<old-base> <new-base> <commit>` 复现那一步（它打印的正是同一批冲突）；
  真有冲突就只能 rebase 到分支提交**直接坐在**新 base 上（重放退化为 no-op，`git diff --stat <old-head> HEAD` 保持为空）。
  无论如何优先用 rebase：它让 PR 的提交列表与正文如实反映改动。
- A genuine content conflict is still possible when two batches touch the same lines (e.g. the migration counters in
  `AGENTS.md`). Resolve by keeping the downstream side, then re-run the zero-drift tests.

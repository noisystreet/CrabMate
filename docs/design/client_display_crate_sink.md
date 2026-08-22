# ADR：展示 crate 下沉 Client（执行计划）

> **状态**：**Proposed**（2026-08-16）；**W3/W4/W5 缓做**（2026-08-16：优先 [`crates_io_single_package.md`](./crates_io_single_package.md)）。  
> **对齐**：[`client_shell_split.md`](./client_shell_split.md) 路径 A（本仓只维护 Server）；[`client_contract_versioning.md`](./client_contract_versioning.md)（线契约钉版本）；Client [`contract_pin.md`](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/contract_pin.md)、[`client_shared_logic.md`](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/client_shared_logic.md)。  
> **Client 仓勾选**：[`crabmate-client/docs/design/display_crate_sink.md`](https://github.com/noisystreet/crabmate-client/blob/main/docs/design/display_crate_sink.md)（消费侧清单；决策以本文为准）。  
> **非目标**：把 `sse-protocol` / `api-contract` / `types` 搬进 Client；Server 依赖 Client git crate。**crates.io 单包**见 [`crates_io_single_package.md`](./crates_io_single_package.md)（不依赖 W3）。

---

## 1. Context

D2.2 已硬删本仓同进程 TUI。官方 UI / `crabmate-tui` 在 Client 仓，却仍 git 钉本仓 **7** 个 crate。其中多数是 **怎么画**（投影、工具卡），不是 **线上是什么字节**。

约束：

1. **依赖单向**：Server 产出 HTTP/SSE；Client 消费。Server **不得**为了编码协议而去依赖 Client 仓。
2. **WASM**：`frontend` 不能链接 `tokio` 运行时 / `nix` / `rusqlite`。
3. **expand/contract**：先双仓都有源码并切依赖，再删本仓副本；禁止大爆炸搬家。
4. **包名不变**：迁出后仍叫 `crabmate-turn-layout` / `crabmate-tool-card`，避免改 `use` 路径。

## 2. Decision

| 包 | 所有权 | 本计划动作 |
|----|--------|------------|
| `crabmate-sse-protocol` | **Server**（编码器 + 分类） | **不下沉**。另轨可切 `protocol`/`runtime` feature（不阻塞本计划）。 |
| `crabmate-api-contract` | **Server**（DTO + OpenAPI） | **不下沉**。Client 去掉对整包的过度依赖（见 W1）。 |
| `crabmate-types` | **Server**（`Message` 领域核） | **不下沉**。Client 只拷网关预设表。 |
| `crabmate-display-rules` | **Server**（注入文案与快照过滤同源） | **不搬源码**。W5 可选：Client 拷 ~97 行到 `crabmate-client-api`，接受双份前缀。 |
| `crabmate-turn-layout` | **本仓**（随单包收成 `protocol` 模块） | **缓做 W3/W4**；crates.io 见 [`crates_io_single_package.md`](./crates_io_single_package.md)。 |
| `crabmate-tool-card` | **Client**（工具卡 UI） | **已迁出**（W2 expand → W2b contract，2026-08-16）。 |
| `crabmate-chat-export` | **Server**（`save-session` raw / `tool-replay`） | **本轮不迁**。Client 继续 git 钉 schema 常量；display Markdown 可后续再拆。 |

**禁止**：Client 再 `path = "../crabmate_agent/crates/…"`（已有 `check-no-main-path.sh`）。expand 阶段 Client 用 **本仓 path**；线契约仍 git tag。

## 3. Consequences

**好处**

- `tool-card` 已离开本仓；单包发布图不再拖 WASM 工具卡 crate（见 [`crates_io_single_package.md`](./crates_io_single_package.md)）。
- （W3 若重启）投影随 UI 发版，不必为改气泡行序打 Server tag。
- Client `frontend` git 依赖从 7 条收到 **线契约 2～3 条**（`sse-protocol` 必留；`api-contract`/`chat-export` 可再瘦）。

**代价**

- `crabmate sse-replay --format canonical` 投影迁走后，本仓只保留 **原文 JSONL 打印**（见 W4）。
- `save-session` Markdown 工具段不再走 Web 像素级 `tool-card`（改用截断原文 / `tool_result` 纯文本）。
- 投影金样改由 Client CI 跑；协议 bump 时须同步更新 Client 的「线→投影」表征夹具。
- `display-rules` 若拷到 Client：注入前缀变更要改两仓（W5 验收写进 PR 说明）。

**后续约束**

- 线协议变更仍以本仓 `docs/SSE协议.md` + `fixtures/sse_*_golden.jsonl` 为权威。
- `docs/Turn布局设计.md` 在 W4 后改为「投影实现在 Client crate；本仓只锁发出的 AG-UI 字节」。

## 4. Alternatives Considered

| 方案 | 否决原因 |
|------|----------|
| 7 包全部迁 Client，Server git 依赖回来 | 依赖反转；`cargo publish crabmate` 绑 UI 仓 |
| 全部并进根包 `crabmate`，Client 依赖**默认** feature | WASM 编不过。**有 `protocol` feature 的单包**见 [`crates_io_single_package.md`](./crates_io_single_package.md) |
| Client 重写 turn-layout / tool-card | ~4500 行 + 金样漂移；无收益 |
| 第三仓 `crabmate-protocol` 只放展示 crate | 三仓发版税；展示本就该跟 UI |

---

## 5. 波次与双仓 PR 顺序

每波：**先 Client PR（能绿）→ 再 Server PR（删/改残留）**，或同一波内 Client 先合、Server 再合。禁止 Server 先删、Client 仍钉旧 tag。

```text
W0 文档（本 ADR）
W1 瘦 Client git 依赖（无搬家）
W2 tool-card expand（Client path）→ W2b Server 去掉依赖
W3 / W4 turn-layout — **缓做**（单包计划将它收成 `crabmate::turn_layout`）
W5 （可选）display-rules 拷贝 — **缓做**
W6 sse-protocol feature — **并入**单包计划 S1（crates_io_single_package.md）
```

钉 tag 的 Client 在 W2 合入前仍可用 `v0.3.0`；合入后 `tool-card` 改 **Client path**。W3 缓做期间 `turn-layout` 继续 git 钉 Server。

---

## 6. W0 — 文档与索引

| ID | 仓 | 动作 | 验收 |
|----|----|------|------|
| W0.1 | Server | 本文 + `client_shell_split_todo` / `开发文档` / `待办清单` 入口 | 本 PR |
| W0.2 | Client | `docs/design/display_crate_sink.md` + `contract_pin.md` 指向本文 | 与 W0.1 同期或紧随 |

---

## 7. W1 — 瘦 Client 依赖（不搬家）

**入口**：W0。**解开**：frontend 不再为「一张表 / 一个 DTO」拉整包 `types` /（尽可能）`api-contract`。

| ID | 仓 | 动作 | 验收 |
|----|----|------|------|
| W1.1 | Client | 把 `llm_gateway_presets` 常量表拷到 `frontend/src/client_llm_presets.rs`（或 `crabmate-client-api`）；去掉 `crabmate-types` 依赖 | `cd frontend && cargo test --lib`；`wasm32` check |
| W1.2 | Client | `StatusShellView`：手写与 OpenAPI 对齐的本地 struct，**或**继续只依赖 `api-contract`（若 W1.2 成本高于收益可 **跳过**，在 PR 注明） | `GET /status?view=shell` 字段仍 `deny_unknown_fields` 可反序列化 |
| W1.3 | Client | 更新 `frontend/Cargo.toml`、`contract_pin.md` 钉清单 | `scripts/check-no-main-path.sh` 仍绿 |

**不改** Server `types` / `api-contract` 源码（除非发现 Client 误用了不该公开的符号）。

### W1 落地（2026-08-16）

- **W1.1**：Client `frontend/src/client_llm_presets.rs` 持有预设表副本；`frontend` 去掉直接 `crabmate-types`。
- **W1.2**：**跳过**。`StatusShellView` 仍走 `crabmate-api-contract`（与 OpenAPI / `deny_unknown_fields` 同源；本地再写一份会漂）。
- **W1.3**：Client `contract_pin.md` 钉清单去掉 `crabmate-types`。lockfile 里它仍可能作为 `api-contract` / `sse-protocol` 的传递依赖出现。

---

## 8. W2 — `crabmate-tool-card` expand

**入口**：W1（可与 W1 并行，但不要与 W3 挤同一个 PR）。

物理：`crates/crabmate-tool-card/`（~2600 行，仅 `serde_json`）→ Client `crates/crabmate-tool-card/`。

| ID | 仓 | 动作 | 验收 |
|----|----|------|------|
| W2.1 | Client | 复制 crate；独立 `[workspace]` + `frontend/Cargo.toml` 改为 `path = "../crates/crabmate-tool-card"` | Client：`cd crates/crabmate-tool-card && cargo test`（无根 workspace，勿用 `-p`）；`cd frontend && cargo test --lib`（工具卡相关） |
| W2.2 | Client | `check-no-main-path.sh` 允许本仓 path、仍禁 Server path | 脚本绿 |
| W2.3 | 双仓 | **expand 窗口**：Server 副本暂留，旧 git tag 消费者不破 | 不删 Server crate |

### W2 expand 落地（2026-08-16）

- **W2.1 / W2.2**：Client `crates/crabmate-tool-card` 为独立 `[workspace]`；`frontend` path 依赖；金样在 crate 内 `fixtures/hydrate_tool_card_golden.jsonl`。
- **W2.3**：本仓 crate **暂留**（`crabmate-runtime` / `save-session` 仍用）；W2b 再删 member 与钉清单。

### W2b — contract（Server 去掉 `tool-card`）

| ID | 仓 | 动作 | 验收 |
|----|----|------|------|
| W2b.1 | Server | `crabmate-runtime` `message_display` / `message_snapshot_display` 不再 `use crabmate_tool_card`；工具 Markdown 用截断原文或现有 `tool_result` 纯文本 | `cargo test -p crabmate-runtime`；`save-session` 仍写出 json/md |
| W2b.2 | Server | 根 `Cargo.toml` members / `workspace.dependencies` 删除；`scripts/check-client-contract.sh` 钉清单去掉 `crabmate-tool-card` | `bash scripts/check-client-contract.sh` |
| W2b.3 | Server | `docs/命令行与路由.md`：说明 `save-session` md 工具段不再保证与 Web 工具卡逐字一致 | 文档与行为一致 |

**产品默认**：运维 CLI 不对齐 Web 像素；需要漂亮工具卡走 Client 导出。

### W2b 落地（2026-08-16）

- **W2b.1**：`tool_content_for_display_for_message` 只用信封摘要 / `summarize_tool_call`；`GET /conversation/messages` 的 `role=tool` **不再**填 `display_*`（Client 水合回退本地 `crabmate-tool-card`）。
- **W2b.2**：本仓删除 `crates/crabmate-tool-card` member；钉清单已同步。圈复杂度门禁现为全仓 **CCN ≤ 10**（`scripts/lizard-rust.sh`），不再使用按模块 caps TOML。
- **W2b.3**：`docs/命令行与路由.md` / `docs/en/CLI.md` 已说明 md/`display` 工具段不与 Web 工具卡逐字对齐。

---

## 9. W3 — `crabmate-turn-layout` expand（**缓做**）

> 2026-08-16：优先单包 crates.io。W3/W4 不再作为下一波；重启前须对照 [`crates_io_single_package.md`](./crates_io_single_package.md)（切仓后本包已是模块，再迁 Client 无益于发布图）。

**入口**：建议 W2b 已合（减并行冲突）；crate **不依赖** `types` / `sse-protocol`（仅 `serde`/`log`），搬家干净。

物理：`crates/crabmate-turn-layout/{lib,event,model,reduce,project,replay}.rs` → Client 同名目录。

金样（随 crate 走，**不要**继续用 `../../fixtures` 指回 Server）：

- `fixtures/turn_project_golden.jsonl`
- `fixtures/turn_project_web_golden.jsonl`
- `fixtures/turn_project_projection_golden.jsonl`

放到 Client `crates/crabmate-turn-layout/fixtures/`（改 `CARGO_MANIFEST_DIR` 拼接）。

| ID | 仓 | 动作 | 验收 |
|----|----|------|------|
| W3.1 | Client | 复制 crate + 三份 jsonl；修正 fixture 路径 | `cargo test -p crabmate-turn-layout golden_turn_project golden_turn_project_web golden_turn_project_projection` |
| W3.2 | Client | `frontend/Cargo.toml` path 依赖；`composer_stream` 等 `use` 不变 | `cd frontend && cargo test --lib golden_turn_web_stored_sync`（及现有 turn_layout 测） |
| W3.3 | Client | CI 增加 `-p crabmate-turn-layout` 金样 job（对标 Server `.github/workflows/code-complexity.yml` 里那条） | Client CI 绿 |
| W3.4 | Client | 可选：从 `sse_ag_ui_golden.jsonl` **摘一小组** AG-UI → `TurnEvent` 表征测，协议 bump 时更新 | 不把整份 Server SSE 金样当 Client 真源 |

expand 窗口内 Server 副本仍可编译 `sse-replay`。

---

## 10. W4 — `turn-layout` contract（本仓收口）

| ID | 仓 | 动作 | 验收 |
|----|----|------|------|
| W4.1 | Server | `crabmate sse-replay`：**默认打印 JSONL `data` 原文**；删除对 `project_turn_web` / `ProjectedRow` 的依赖。`--format canonical` **删除或文档化为已移除** | `cargo test` 覆盖 cli 解析；手动 `sse-replay` 仍能读 dump 文件 |
| W4.2 | Server | 移出 workspace member；`check-client-contract.sh` 去掉该包；删除根 `fixtures/turn_project_*.jsonl`（已在 Client） | 门禁绿；无悬空路径 |
| W4.3 | Server | 更新 `docs/Turn布局设计.md`、`docs/开发文档.md`、`docs/en/DEVELOPMENT.md`：投影权威 = Client crate；本仓金样只锁 SSE 字节 | 链接指向 Client 路径 |
| W4.4 | Server | `client_contract_versioning.md` §4.1「官方 UI 展示契约」删 `turn-layout`（`tool-card` 已在 W2b 去掉）；保留线契约四件套（+ 仍钉的 `chat-export` / `display-rules` 直至 W5） | 外仓示例 toml 与 `check-client-contract.sh` 一致 |

**回滚**：expand 窗口未关时可把 Client 改回 git tag；W4 合入后回滚需还原 Server member（保留 git 历史）。

---

## 11. W5 — （可选）`display-rules` 拷贝

**不搬 crate**（Server `types` 快照过滤仍要用）。

| ID | 仓 | 动作 | 验收 |
|----|----|------|------|
| W5.1 | Client | 将 `user_message_should_hide_for_chat_display` 等函数拷入 `crabmate-client-api`；`session_merge` / `message_ex` 改用它 | 与 `fixtures/display_hide_user_golden.jsonl` **逐行对照**（可在 Client 放一份拷贝金样） |
| W5.2 | Client | `frontend` 去掉 `crabmate-display-rules` git 依赖 | wasm check |
| W5.3 | 双仓 | PR 说明：改注入前缀须 **两仓同 PR 窗口**（或 Server 先发、Client 跟） | 清单写进 `types` / `display-rules` 模块注释 |

若双份维护不可接受：保持 git 钉，**跳过 W5**。

---

## 12. W6 — （另轨，不阻塞）`sse-protocol` feature

为日后 crates.io 薄协议包：`default = ["protocol"]`，`runtime` 才启用 `tokio` hub / 审批 mpsc。Client 只开 `protocol`。

本计划 **不要求** W6 完成才能合 W4。

---

## 13. 明确不做

- 不迁 `crabmate-chat-export`（依赖 `types::Message`；`save-session --projection raw` 是 Server 运维契约）。
- 不让 Server `Cargo.toml` `git = crabmate-client`。
- 不把 `Turn布局设计.md` 整篇搬到 Client（人读协议仍在本仓；实现指针改 Client）。
- 不等待 `client-contract-v*` 新 tag 才开始 W2（展示 crate 将离开该 tag 清单）。

---

## 14. 验证矩阵（每波结束）

| 检查 | 命令 / 位置 |
|------|-------------|
| Server 线契约 | `bash scripts/check-client-contract.sh` |
| Server 全量（W2b/W4 后） | `cargo test`（至少 `crabmate` + `crabmate-runtime`） |
| Client 展示 crate | Client：`cd crates/crabmate-tool-card && cargo test` / `cd crates/crabmate-turn-layout && cargo test` |
| Client WASM | `cd frontend && cargo test --lib`；既有 `make frontend-check` |
| 禁 path | Client `scripts/check-no-main-path.sh` |
| 一轮对话 | 既有 Client 冒烟 / Playwright（投影回归以金样为主，不必每波真 LLM） |

---

## 15. 与待办 B2（E2）的关系

`docs/待办清单.md` **B2**：服务端可选布局元数据 + 共享投影 golden。

本 ADR **不实现 B2**。W4 之后「共享投影 golden」的物理位置在 **Client**；若仍要做 B2，改为 Server 下发可选元数据、Client 投影消费，而不是把 `turn-layout` 留在本仓。

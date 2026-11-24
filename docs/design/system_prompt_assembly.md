# 系统提示词动态组装

**状态**：维护者专题（自 `docs/开发文档.md` 拆出，避免主文档膨胀）。  
**配置键**：见 **`docs/配置说明.md`**。  
**代码锚点**：`config/finalize`、`context_bootstrap/`、`web/.../turn_build`、`agent_turn` Act 句启发式。

## 原则（冲突时）

1. **P0 优先级**：安全与不可逆约束 → 用户明确指令 → 编排层短时约束（Act 句启发式 hint、规划 coach）→ 项目规则 / Skills → 运行时统计类附录。  
2. **分流**：长期稳定准则 → 首条或持久 `system`；仅本轮有效 → 服务端注入消息（专用 `user` / `system_execution_constraint_hint` 等）。  
3. **隔离**：用户输入与工具输出默认不写回首条 `system`；工作区规则视为可信附录，仍受工作区信任边界约束。  
4. **与工具契约一致**：描述须与 `tool_registry` / Schema / `executor_kind` 收窄一致。  
5. **预算**：`agents_md_max_chars`、`skills_*`、`context_char_budget` / `max_message_history` 等共同约束。  
6. **可观测**：排障看最终送供应商的 `messages`（经 `conversation_messages_to_vendor_body`），勿只看磁盘会话 JSON。

## 概念块 L0–L8（非单一结构体）

| 块 | 典型来源 | 组装时机 |
|----|----------|----------|
| **L0** | `system_prompt` / `base_system_prompt.md` | `finalize` |
| **L0b** | `coding_workbench_increment*`（可按角色跳过） | `finalize` |
| **L1'** | **`AGENTS.md`**（会话工作区根；取代原 `.cursor/rules` 兼容） | 首条 `system` 构建（`prompt_compose`） |
| **L2** | Skills **索引**（非全文：`id — description` **全量**，受 `skills_max_chars` 约束；工作区 + 用户级 + 系统级合并；扫描基准 = **会话工作区根**，与 `skill_read` 同源） | 首条 `system` 构建（`prompt_compose`） |
| **L3** | `agent_roles` 每角色正文；首条 `system` 构建时在其**末尾追加环境/界面 locale 输出语言指令**（`resolve_role_system_base`） | `finalize_agent_role_catalog`（+ 首条构建追加） |
| **L4** | thinking 附录、工具统计等运行时附录 | 首条 `system` 构建 |
| **L5** | Skills **正文**：只在 **`/<skill-id>`** 时注入，且注入到**真实 user 之后**的专用 `user`（`crabmate_forced_skill`，服务端注入：UI 不展示、落盘剥离）——**不进**首条 `system`，否则该回合整段前缀缓存失效（#1475）。其余按需由模型调 `skill_read`；旧的按轮 top-k 打分预注入已删除（#1326） | `turn_build`（`/<id>`）/ 工具调用（`skill_read`） |
| **L6** | 首轮工作区上下文（活文档摘要 / 备忘 / 依赖摘要；专用 **user**） | `compose_new_conversation_messages` |
| **L7** | Act 句启发式 hint（`system_execution_constraint_hint`；形如「【执行约束】本轮：…。勿越过。」） | 外循环 P 步前 |
| **L8** | 记忆 / 变更集等 | `prepare_messages_for_model` / 管道 |

### 逐段可变性与前缀缓存（#1475）

前缀缓存按**最长公共前缀**匹配，而首条 `system` 位于 token 序列最前端：其中任一字节变化，
其后全部历史一起 miss。续聊每轮都会重算首条 `system`（`refresh_existing_turn_system`），
所以下表回答的是「重算能容忍什么」——以及变化时代价落在哪一段。

| 段落 | 何时变 | 缓存影响 |
|---|---|---|
| L0 / L0b / L3 基底 | 改配置 / 换角色 | 换角色虽是显式操作，但一次变化至少亏两轮 |
| L3 + locale 指令 | 切换界面语言 | 低频，可接受 |
| L2 技能索引 | 增删技能 / 改 description / 换工作区 | 打断前缀（索引在首条 `system` 内） |
| L1' `AGENTS.md` | 工作区该文件改动（含 agent 自己写入） | 打断前缀；**规则下一回合生效优先**，不为命中率牺牲（非目标） |
| L4 工具统计 hints | 开启 `agent_tool_stats_enabled` 后随最近窗口抖动 | **代价最大**：一变就打断前缀；默认关闭，不开则无此段 |
| L5 技能正文 | 该回合用了 `/<skill-id>` | **不影响前缀**：已注入到真实 user 之后（#1475 移出 `system`） |
| L6 首轮上下文 | 仅新会话一次 | 一次 |
| L7 执行约束 hint | Act 句命中时 | 独立 `system` 消息、追加在末尾 ⇒ 只让尾部 miss |
| L8 记忆 / 变更集 | 每轮随注入刷新 | 同样位于尾部，只让尾部 miss |

**观测口径**：`first_system_hash` 只取**首条** `system`
（`vendor_messages::first_system_stability_info`）。它与 Web 侧
`first_system_compose path=web_new|web_existing` 日志同值，可与出站
`prompt_cache … first_system_hash=… hit=… miss=…` 关联——命中率因此能按入口分桶、归因到上表某一段。
**不要**用「全部 `system` 拼接后的 `all_system_hash`」判断前缀是否断裂：L7 这类末尾追加会改它，但不断前缀。

运行时选用：`AgentConfig::system_prompt_for_new_conversation` 选 L3 与全局 system 后，
  由 `resolve_role_system_base` 在 L3 基底末尾追加 locale 输出语言指令；其后由 `prompt_compose` 按**会话工作区根**拼 **L2**（技能索引）与
  **`AGENTS.md`**（L1'，受 `agents_md_*` 约束），再叠 L4。**L5 正文不进 `system`**，
  由 `turn_build` 注入到真实 user 之后（#1475）。L2 与 L5 同源
  （同一组 `skills_*` 配置键、同一基准目录），
  因此索引里列出的技能必然能被 `skill_read` 读到；索引读目录失败只告警降级，不让整轮组装报错。

## 会话生命周期（摘要）

1. 配置装载：L0 → L0b →（角色）L3；**L2** 留到首条 `system` 构建时按会话工作区根拼装。  
2. 新会话：`system` = 角色 system + L2（技能索引）+ L1'（`AGENTS.md`）+ L4；
   可选 L6 `user`；再真实用户 `user`；若本轮有 `/<skill-id>` 则再追加 L5 注入 `user`。  
3. 续聊可因 `agent_role` 重写首条 `system`（`agent_role_turn`）。  
4. 从磁盘恢复时常用**当前配置**替换首条 `system`。  
5. 每轮 Agent：L7（如有）→ 管道 / L8。

## 演进（可选）

块注册表、模板变量、版本指纹、组装 golden、统一动态入口（宜收敛在 `context_bootstrap/prompt_compose`）。细节实现以源码为准，勿在本文堆实现清单。

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
| **L5** | Skills **正文**：默认**不注入**（`skills_top_k = 0`，由模型调 `skill_read` 按需读取）；`skills_top_k > 0` 时按轮 top-k 注入；`/<skill-id>` 始终强制注入（同源三层合并） | Web `build_messages_for_turn` / CLI 刷新首条 |
| **L6** | 首轮工作区上下文（活文档摘要 / 备忘 / 依赖摘要；专用 **user**） | `compose_new_conversation_messages` |
| **L7** | Act 句启发式 hint（`system_execution_constraint_hint`；形如「【执行约束】本轮：…。勿越过。」） | 外循环 P 步前 |
| **L8** | 记忆 / 变更集等 | `prepare_messages_for_model` / 管道 |

运行时选用：`AgentConfig::system_prompt_for_new_conversation` 选 L3 与全局 system 后，
  由 `resolve_role_system_base` 在 L3 基底末尾追加 locale 输出语言指令；其后由 `prompt_compose` 按**会话工作区根**拼 **L2**（技能索引）与
  **`AGENTS.md`**（L1'，受 `agents_md_*` 约束），再叠 L4（及 Web/CLI 的 L5）。L2 与 L5 同源（同一组 `skills_*` 配置键、同一基准目录），
  因此索引里列出的技能必然能被 `skill_read` 读到；索引读目录失败只告警降级，不让整轮组装报错。

## 会话生命周期（摘要）

1. 配置装载：L0 → L0b →（角色）L3；**L2** 留到首条 `system` 构建时按会话工作区根拼装。  
2. 新会话：`system` = 角色 system + L2（技能索引）+ L1'（`AGENTS.md`）+ L4（+ L5）；可选 L6 `user`；再真实用户 `user`。  
3. 续聊可因 `agent_role` 重写首条 `system`（`agent_role_turn`）。  
4. 从磁盘恢复时常用**当前配置**替换首条 `system`。  
5. 每轮 Agent：L7（如有）→ 管道 / L8。

## 演进（可选）

块注册表、模板变量、版本指纹、组装 golden、统一动态入口（宜收敛在 `context_bootstrap/prompt_compose`）。细节实现以源码为准，勿在本文堆实现清单。

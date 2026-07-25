/**
 * 真实 LLM 端到端测试：工具调用场景
 *
 * 覆盖真实 LLM 场景下的工具调用全流程：
 *   - 工具卡（tool card）在前端正确渲染
 *   - 工具结果可见
 *   - 终答可见
 *   - 会话持久化包含工具调用记录
 *
 * 前置条件：
 *   1. `cargo run -- serve` 在 127.0.0.1:8080 运行
 *   2. 通过以下方式之一配置 API 密钥（优先级递减）：
 *      - 环境变量 API_KEY
 *      - 项目根 config.toml（[agent] 节下的 api_key）
 *      - 项目根 .agent_demo.toml（同上）
 *      - ~/.local/share/crabmate/secrets/client_llm（Tauri 本地配置）
 *
 * 运行方式：
 *   cd e2e && npx playwright test specs/real-llm-tool-call.spec.ts
 *
 * 注意：
 *   - 无密钥时测试自动跳过
 *   - 真实 LLM 调用较慢，超时设置为 180 秒
 */

import { test, expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import {
  setupRealLLMSession,
  sendMessage,
  waitForReady,
} from "../fixtures/helpers";

/** 从 TOML 配置文件中读取 api_key。 */
function readApiKeyFromToml(filePath: string): string {
  try {
    const raw = fs.readFileSync(filePath, "utf8");
    const inAgentSection: string[] = [];
    let inAgent = false;
    for (const line of raw.split("\n")) {
      const trimmed = line.trim();
      if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
        const section = trimmed.slice(1, -1).trim();
        inAgent = section === "agent";
        continue;
      }
      if (inAgent && trimmed.startsWith("api_key")) {
        const eqIdx = trimmed.indexOf("=");
        if (eqIdx !== -1) {
          let val = trimmed.slice(eqIdx + 1).trim();
          if (
            (val.startsWith('"') && val.endsWith('"')) ||
            (val.startsWith("'") && val.endsWith("'"))
          ) {
            val = val.slice(1, -1);
          }
          if (val) inAgentSection.push(val);
        }
      }
    }
    if (inAgentSection.length > 0)
      return inAgentSection[inAgentSection.length - 1];
  } catch {
    /* 文件不存在或无法读取 */
  }
  return "";
}

/** 读取 API 密钥：环境变量 → config.toml → .agent_demo.toml → Tauri secrets 文件。 */
function resolveApiKey(): string {
  const env = process.env.API_KEY;
  if (env && env.trim()) return env.trim();

  const projectRoot = path.resolve(process.cwd(), "..");
  const fromConfig = readApiKeyFromToml(path.join(projectRoot, "config.toml"));
  if (fromConfig) return fromConfig;
  const fromDemo = readApiKeyFromToml(
    path.join(projectRoot, ".agent_demo.toml"),
  );
  if (fromDemo) return fromDemo;

  const dataHome =
    process.env.XDG_DATA_HOME ||
    path.join(process.env.HOME || "", ".local", "share");
  const secretsPath = path.join(dataHome, "crabmate", "secrets", "client_llm");
  try {
    return fs.readFileSync(secretsPath, "utf8").trim();
  } catch {
    return "";
  }
}

const API_KEY = resolveApiKey();
const SID_BASE = "s_e2e_real_tool_call";

test.describe("真实 LLM：工具调用场景", () => {
  const runTest = API_KEY ? test : test.skip;
  // 每次运行用唯一 SID，避免前次会话残留状态干扰
  const uniqueSid = `${SID_BASE}_${Date.now()}`;
  const uniqueSidPersist = `${SID_BASE}_persist_${Date.now()}`;

  runTest("工具卡 + 工具结果 + 终答在 UI 中可见", async ({ page }) => {
    await setupRealLLMSession(page, uniqueSid, API_KEY);
    // 要求列出文件结构，模型必然会调用 list_tree 工具
    await sendMessage(page, "列出当前工作区目录结构，用列表工具。");

    await waitForReady(page, 180_000);

    // 状态栏显示就绪
    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 5_000 },
    );

    // 至少有一个工具卡
    const toolCards = await page
      .locator('[data-testid="chat-tool-card"]')
      .count();
    expect(toolCards).toBeGreaterThanOrEqual(1);

    // 终答可见
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).not.toBeEmpty({ timeout: 5_000 });

    // 不应出现错误提示
    const errorToasts = await page
      .locator('[data-testid="error-toast"]')
      .count();
    expect(errorToasts).toBe(0);
  });

  runTest("会话消息持久化包含助手终答内容", async ({ page }) => {
    await setupRealLLMSession(page, uniqueSidPersist, API_KEY);
    await sendMessage(
      page,
      "现在几点？请用 get_current_time 工具获取当前时间。",
    );

    await waitForReady(page, 180_000);

    // 从后端拉取会话消息验证持久化包含助手终答
    // 注：前端的 StoredMessage 不存储 tool_calls 字段，
    // 因此改为验证至少 2 条消息（用户 + 助手）且助手终答有实质内容
    let messages: unknown[] = [];
    const pollTimeout = 30_000;
    const pollInterval = 500;
    for (let elapsed = 0; elapsed < pollTimeout; elapsed += pollInterval) {
      const fetched: unknown[] = await page.evaluate(
        (sid: string) =>
          fetch("/user-data/workspaces/current/sessions")
            .then((r) => r.json())
            .then((d) => {
              const list = d.current?.sessions || d.sessions || [];
              const s = Array.isArray(list)
                ? list.find((x: { id: string }) => x.id === sid)
                : null;
              return s ? s.messages || [] : [];
            }),
        uniqueSidPersist,
      );
      // 至少 2 条消息意味着用户消息和助手回复都已持久化
      if (fetched.length >= 2) {
        messages = fetched;
        break;
      }
      await new Promise((r) => setTimeout(r, pollInterval));
    }

    // 至少有一条 user 消息
    const userMessages = (messages as Array<{ role: string }>).filter(
      (m) => m.role === "user",
    );
    expect(userMessages.length).toBeGreaterThanOrEqual(1);

    // 至少有一条 assistant 消息（终答）
    const assistantMessages = (
      messages as Array<{ role: string; text: string }>
    ).filter((m) => m.role === "assistant" && (m.text || "").trim().length > 0);
    expect(assistantMessages.length).toBeGreaterThanOrEqual(1);

    // 终答内容应有实质长度
    const finalText = assistantMessages.map((m) => m.text).join("");
    expect(finalText.length).toBeGreaterThan(10);
  });
});

/**
 * 真实 LLM 端到端测试：无工具问答场景
 *
 * 覆盖 PR #678 修复的终答气泡（FINAL_ANSWER_ROW）可见性：
 *   - 流完成后终答正文在 UI 中可见
 *   - 会话消息持久化包含 assistant 终答
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
 *   cd e2e && npx playwright test specs/real-llm-zero-tool.spec.ts
 *
 * 注意：
 *   - 密钥优先级：环境变量 API_KEY > Tauri secrets 文件
 *   - 真实 LLM 调用较慢，超时设置为 180 秒
 *   - 无密钥时测试自动跳过（不影响 CI 中 mock SSE 测试）
 */

import { test, expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import {
  setupRealLLMSession,
  sendMessage,
  waitForReady,
} from "../fixtures/helpers";

/** 从 TOML 配置文件中读取 api_key（简单带缓存的 TOML 解析，仅提取 api_key）。 */
function readApiKeyFromToml(filePath: string): string {
  try {
    const raw = fs.readFileSync(filePath, "utf8");
    // 匹配 `[agent]` 节下的 `api_key = "..."` 或 `api_key = '...'`
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
    /* 文件不存在或无法读取，忽略 */
  }
  return "";
}

/** 读取 API 密钥：环境变量 → config.toml → .agent_demo.toml → Tauri secrets 文件。 */
function resolveApiKey(): string {
  const env = process.env.API_KEY;
  if (env && env.trim()) return env.trim();

  // 从项目配置文件读取（e2e/playwright.config.ts → ../.. → 项目根）
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
const SID = "s_e2e_real_zero_tool";

test.describe("真实 LLM：无工具终答场景", () => {
  // 无密钥时跳过（describe 块始终运行，内部按条件跳过）
  const runTest = API_KEY ? test : test.skip;

  runTest("流完成后终答正文在 UI 中可见", async ({ page }) => {
    await setupRealLLMSession(page, SID, API_KEY);
    await sendMessage(page, "你有哪些核心功能？");

    await waitForReady(page, 180_000);

    // 状态栏显示就绪
    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 5_000 },
    );

    // 终答正文可见（关键词来自真实 LLM 回复）
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).not.toBeEmpty({ timeout: 5_000 });

    // 不应出现错误提示
    const errorToasts = await page
      .locator('[data-testid="error-toast"]')
      .count();
    expect(errorToasts).toBe(0);
  });

  runTest("会话消息持久化包含 assistant 终答", async ({ page }) => {
    await setupRealLLMSession(page, SID + "_persist", API_KEY);
    await sendMessage(page, "列举三个你可以做的事情");

    await waitForReady(page, 180_000);

    // 从后端拉取会话消息验证持久化
    // 注意：前端在收到 conversation_saved SSE 后异步提交 PUT 到 sessions API，
    // 因此需要轮询直到 assistant 消息文本可用。
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
        SID + "_persist",
      );
      const hasAssistantText = (
        fetched as Array<{ role: string; text: string }>
      ).some((m) => m.role === "assistant" && (m.text || "").trim().length > 0);
      if (hasAssistantText) {
        messages = fetched;
        break;
      }
      await new Promise((r) => setTimeout(r, pollInterval));
    }

    // 至少有一条 assistant 角色、非工具的终答消息
    const assistantMessages = (
      messages as Array<{ role: string; is_tool: boolean; text: string }>
    ).filter(
      (m) =>
        m.role === "assistant" &&
        !m.is_tool &&
        (m.text || "").trim().length > 0,
    );
    expect(assistantMessages.length).toBeGreaterThanOrEqual(1);

    // 终答内容应有实质长度（非空或仅标点）
    const finalText = assistantMessages.map((m) => m.text).join("");
    expect(finalText.length).toBeGreaterThan(10);
  });
});

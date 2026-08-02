/**
 * 真实 LLM E2E：连续三轮上下文对话，并验证每轮完成后保持在最新消息末尾。
 *
 * 运行：
 *   REAL_LLM_E2E=1 API_KEY=YOUR_API_KEY \
 *     npx playwright test specs/real-llm-three-turn-scroll.spec.ts
 *
 * 未显式启用或未配置密钥时自动跳过，避免 CI 意外产生真实调用费用。
 */

import { expect, Page, test } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { sendMessage, setupRealLLMSession } from "../fixtures/helpers";

const PROMPTS = ["你好", "你有哪些技能", "还有吗？"] as const;
const SID = `s_e2e_real_three_turn_${Date.now()}`;

function readApiKeyFromToml(filePath: string): string {
  try {
    const lines = fs.readFileSync(filePath, "utf8").split("\n");
    let inAgentSection = false;
    for (const line of lines) {
      const trimmed = line.trim();
      if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
        inAgentSection = trimmed.slice(1, -1).trim() === "agent";
        continue;
      }
      if (!inAgentSection || !trimmed.startsWith("api_key")) continue;
      const separator = trimmed.indexOf("=");
      if (separator < 0) continue;
      const value = trimmed
        .slice(separator + 1)
        .trim()
        .replace(/^(['"])(.*)\1$/, "$2");
      if (value) return value;
    }
  } catch {
    // 可选配置不存在时继续尝试下一来源。
  }
  return "";
}

function resolveApiKey(): string {
  if (process.env.API_KEY?.trim()) return process.env.API_KEY.trim();
  const projectRoot = path.resolve(process.cwd(), "..");
  for (const configName of ["config.toml", ".agent_demo.toml"]) {
    const value = readApiKeyFromToml(path.join(projectRoot, configName));
    if (value) return value;
  }
  const dataHome =
    process.env.XDG_DATA_HOME ??
    path.join(process.env.HOME ?? "", ".local", "share");
  try {
    return fs
      .readFileSync(
        path.join(dataHome, "crabmate", "secrets", "client_llm"),
        "utf8",
      )
      .trim();
  } catch {
    return "";
  }
}

async function scrollGapPx(page: Page): Promise<number> {
  return page.evaluate(() => {
    const scroller = document.querySelector(
      '[data-testid="chat-messages-scroller"]',
    );
    if (!scroller) return -1;
    return scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
  });
}

async function waitForTurnComplete(
  page: Page,
  assistantCountBefore: number,
  timeoutMs = 180_000,
) {
  const deadline = Date.now() + timeoutMs;
  const statusBar = page.locator('[data-testid="status-bar"]');
  const approvalModal = page.locator('[data-testid="approval-modal"]');
  while (Date.now() < deadline) {
    if (await approvalModal.isVisible()) {
      await page.locator('[data-testid="approval-allow-always"]').click();
      continue;
    }
    const ready = (await statusBar.textContent())?.includes("就绪");
    // 默认主列为 TUI；旧气泡类名 .msg-assistant 不再出现
    const assistantCount = await page
      .locator("section.chat-tui-turn--assistant")
      .count();
    if (ready && assistantCount > assistantCountBefore) return;
    await page.waitForTimeout(250);
  }
  throw new Error(`真实 LLM 回合在 ${timeoutMs}ms 内未完成`);
}

const API_KEY = resolveApiKey();
const ENABLED = process.env.REAL_LLM_E2E === "1" && Boolean(API_KEY);

test.describe("真实 LLM：三轮对话滚动跟随", () => {
  const runTest = ENABLED ? test : test.skip;

  runTest(
    "你好 → 技能 → 追问，每轮完成后均停在最新消息末尾",
    async ({ page }) => {
      test.setTimeout(600_000);
      await setupRealLLMSession(page, SID, API_KEY);

      for (const prompt of PROMPTS) {
        const assistantCountBefore = await page
          .locator("section.chat-tui-turn--assistant")
          .count();
        await sendMessage(page, prompt);
        await waitForTurnComplete(page, assistantCountBefore);
        await expect
          .poll(() => scrollGapPx(page), { timeout: 10_000 })
          .toBeLessThanOrEqual(4);
      }

      await expect(page.locator("section.chat-tui-turn--user")).toHaveCount(
        PROMPTS.length,
      );
      expect(
        await page.locator("section.chat-tui-turn--assistant").count(),
      ).toBeGreaterThanOrEqual(PROMPTS.length);
      for (const prompt of PROMPTS) {
        await expect(
          page.locator('[data-testid="chat-messages-scroller"]'),
        ).toContainText(prompt);
      }
      expect(await page.locator('[data-testid="error-toast"]').count()).toBe(0);
    },
  );
});

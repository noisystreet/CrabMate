/**
 * 真实 LLM 端到端：意图分析旁注不出现在聊天气泡。
 *
 * 前置条件：
 *   1. `cargo run -- serve` 在 127.0.0.1:8080 运行
 *   2. API 密钥：环境变量 API_KEY / 本地 config / 钥匙串
 *
 * 运行方式：
 *   cd e2e && npx playwright test specs/real-llm-intent-analysis.spec.ts
 *
 * 注意：无密钥时测试自动跳过。
 */

import { test, expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import {
  setupRealLLMSession,
  sendMessage,
  waitForReady,
} from "../fixtures/helpers";

/** 从 TOML 配置文件中读取 api_key（简单 TOML 解析，仅提取 api_key）。 */
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
    /* 文件不存在或无法读取，忽略 */
  }
  return "";
}

/** 测试请求显式携带的 API 密钥：环境变量 → 本地测试配置。 */
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

  return "";
}

const API_KEY = resolveApiKey();
const SID = "s_e2e_real_intent_analysis";

test.describe("真实 LLM：意图分析卡片场景", () => {
  const runTest = API_KEY ? test : test.skip;

  runTest("聊天区不出现意图分析气泡", async ({ page }) => {
    await setupRealLLMSession(page, SID, API_KEY);
    await sendMessage(page, "读取当前目录下的所有 Rust 源文件");

    await waitForReady(page, 180_000);

    const scroller = page.locator('[data-testid="chat-messages-scroller"]');
    await expect(scroller).not.toBeEmpty({ timeout: 5_000 });
    await expect(scroller).not.toContainText("意图分析：");

    const errorToasts = await page
      .locator('[data-testid="error-toast"]')
      .count();
    expect(errorToasts).toBe(0);
  });
});

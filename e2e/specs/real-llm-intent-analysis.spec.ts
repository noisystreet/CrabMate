/**
 * 真实 LLM 端到端测试：意图分析消息卡片可见性
 *
 * 复现 Bug：分阶段规划（staged_plan）移除后，后端不再发射 intent_analysis
 * timeline_log SSE 事件，导致前端不渲染意图分析消息卡片。
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
 *   cd e2e && npx playwright test specs/real-llm-intent-analysis.spec.ts
 *
 * 注意：
 *   - 无密钥时测试自动跳过
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

/** 读取 API 密钥：环境变量 → config.toml → .agent_demo.toml → Tauri secrets 文件。 */
function resolveApiKey(): string {
  const env = process.env.API_KEY;
  if (env && env.trim()) return env.trim();

  // 从项目配置文件读取
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
const SID = "s_e2e_real_intent_analysis";

test.describe("真实 LLM：意图分析卡片场景", () => {
  const runTest = API_KEY ? test : test.skip;

  runTest("意图分析消息卡片在聊天区可见", async ({ page }) => {
    await setupRealLLMSession(page, SID, API_KEY);
    await sendMessage(page, "读取当前目录下的所有 Rust 源文件");

    await waitForReady(page, 180_000);

    // 验证意图分析卡片出现（包含 "意图分析：" 前缀）
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText("意图分析：", { timeout: 5_000 });

    // 验证卡片包含分析结果中的关键行
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText("主意图：", { timeout: 3_000 });

    // 终答复应也可见
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).not.toBeEmpty({ timeout: 5_000 });

    // 不应出现错误提示
    const errorToasts = await page
      .locator('[data-testid="error-toast"]')
      .count();
    expect(errorToasts).toBe(0);
  });
});

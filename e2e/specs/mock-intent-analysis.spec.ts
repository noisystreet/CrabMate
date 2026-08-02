/**
 * Mock SSE 回归：意图分析旁注不再渲染为聊天气泡。
 *
 * 运行方式（前置：`cargo run -- serve` 在 127.0.0.1:8080 运行）：
 *   cd e2e && npx playwright test specs/mock-intent-analysis.spec.ts
 */

import { test, expect } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const SID = "s_e2e_mock_intent";

/** 构造含 intent_analysis 的 SSE 流（前端应忽略该 timeline_log）。 */
function sseWithIntent(): string {
  const intentLogLine = JSON.stringify({
    type: "CUSTOM",
    customType: "timeline_log",
    data: {
      kind: "intent_analysis",
      title: "意图分析：执行类（直接执行）",
      detail:
        "主意图：execute.run_test_build\n综合置信度：0.61\n需要澄清：否\n决策来源：L2",
    },
  });
  const answer = "这是带意图分析的测试回复。";
  return [
    `id: 1\ndata: ${intentLogLine}\n\n`,
    `id: 2\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n`,
    `id: 3\ndata: ${answer}\n\n`,
    `id: 4\ndata: {"type":"RUN_FINISHED"}\n\n`,
  ].join("");
}

function installSseRoute(page: import("@playwright/test").Page, body: string) {
  return page.route("**/chat/stream", (route) => {
    if (route.request().method() !== "POST") return route.continue();
    return route.fulfill({
      status: 200,
      headers: {
        "content-type": "text/event-stream; charset=utf-8",
        "x-conversation-id": "e2e-intent",
        "x-stream-job-id": "1",
      },
      body,
    });
  });
}

test.describe("意图分析卡片回归", () => {
  test("SSE 含 intent_analysis → 聊天区不出现意图气泡", async ({ page }) => {
    await installSseRoute(page, sseWithIntent());
    await seedSession(page, SID);
    await sendMessage(page, "测试");

    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );

    const scroller = page.locator('[data-testid="chat-messages-scroller"]');
    await expect(scroller).toContainText("这是带意图分析的测试回复。", {
      timeout: 3000,
    });
    await expect(scroller).not.toContainText("意图分析：执行类（直接执行）");
    await expect(scroller).not.toContainText("主意图：execute.run_test_build");
  });
});

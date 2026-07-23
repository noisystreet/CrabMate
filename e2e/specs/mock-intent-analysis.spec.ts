/**
 * Mock SSE 回归测试：意图分析消息卡片
 *
 * 复现 Bug：分阶段规划（staged_plan）移除后，后端不再发射 intent_analysis
 * timeline_log SSE 事件，导致前端不渲染意图分析消息卡片。
 *
 * 测试矩阵：
 *   1. 正向测试：SSE 含 intent_analysis → 卡片可见（前端能力验证）
 *   2. 负向测试：SSE 不含 intent_analysis → 卡片不可见（当前后端行为复现）
 *
 * 运行方式（前置：`cargo run -- serve` 在 127.0.0.1:8080 运行）：
 *   cd e2e && npx playwright test specs/mock-intent-analysis.spec.ts
 */

import { test, expect } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const SID = "s_e2e_mock_intent";

/** 构造含 intent_analysis 的 SSE 流（通过=正向测试，修复后后端应与此一致）。 */
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

/** 构造不含 intent_analysis 的 SSE 流（负向=复现当前后端行为）。 */
function sseWithoutIntent(): string {
  const answer = "这是不带意图分析的测试回复。";
  return [
    `id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n`,
    `id: 2\ndata: ${answer}\n\n`,
    `id: 3\ndata: {"type":"RUN_FINISHED"}\n\n`,
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
  test("正向：SSE 含 intent_analysis → 卡片可见", async ({ page }) => {
    await installSseRoute(page, sseWithIntent());
    await seedSession(page, SID);
    await sendMessage(page, "测试");

    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );

    // 卡片标题
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText("意图分析：执行类（直接执行）", { timeout: 5000 });
    // 卡片细节
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText("主意图：execute.run_test_build", { timeout: 3000 });
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText("综合置信度：0.61", { timeout: 3000 });
    // 终答正常
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText("这是带意图分析的测试回复。", { timeout: 3000 });
  });

  test("负向（复现 Bug）：SSE 不含 intent_analysis → 卡片不可见", async ({
    page,
  }) => {
    await installSseRoute(page, sseWithoutIntent());
    await seedSession(page, SID + "_neg");
    await sendMessage(page, "测试");

    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );

    // 意图分析卡片不应出现
    const scroller = page.locator('[data-testid="chat-messages-scroller"]');
    await expect(scroller).not.toContainText("意图分析：", { timeout: 3000 });

    // 终答仍然正常
    await expect(scroller).toContainText("这是不带意图分析的测试回复。", {
      timeout: 3000,
    });
  });
});

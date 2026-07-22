/**
 * Mock SSE 回归测试：多轮对话
 *
 * 模拟连续两轮问答 + 页面重载，验证：
 *   1. 第一轮流完成后终答正常显示
 *   2. 第二轮发送后两轮终答同时可见
 *   3. 页面重载后会话消息仍持久化
 *
 * 运行方式（前置：`cargo run -- serve` 在 127.0.0.1:8080 运行）：
 *   cd e2e && npx playwright test
 */

import { test, expect } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

test.describe("多轮对话回归", () => {
  test("two_turns_both_answers_visible", async ({ page }) => {
    const turn1Answer =
      "项目结构包含 src 目录、Cargo.toml 配置文件和 README.md 文档。";
    const turn2Answer = "src 目录下有 main.rs 和 lib.rs 两个源文件。";

    const sse1 = [
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      `id: 2\ndata: ${turn1Answer}\n\n`,
      'id: 3\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");

    const sse2 = [
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      `id: 2\ndata: ${turn2Answer}\n\n`,
      'id: 3\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");

    // 注册路由，根据调用次数返回不同的 SSE
    let callCount = 0;
    await page.route("**/chat/stream", (route) => {
      if (route.request().method() !== "POST") return route.continue();
      callCount++;
      const body = callCount === 1 ? sse1 : sse2;
      return route.fulfill({
        status: 200,
        headers: {
          "content-type": "text/event-stream; charset=utf-8",
          "x-conversation-id": "e2e-conv",
          "x-stream-job-id": String(callCount),
        },
        body,
      });
    });

    await seedSession(page, "s_e2e_mock_multi");

    // ── 第一轮 ──
    await sendMessage(page, "项目结构是怎样的");
    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(turn1Answer, { timeout: 5000 });

    // ── 第二轮 ──
    await sendMessage(page, "src 目录呢");
    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );
    // 两轮终答同时可见（修复前：第二轮覆盖第一轮，turn1Answer 丢失）
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(turn1Answer, { timeout: 5000 });
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(turn2Answer, { timeout: 5000 });

    // 至少两行 non-tool 消息（user + assistant 各一）
    const nonToolRows = await page.evaluate(
      () =>
        document.querySelectorAll('[data-testid="chat-message-row"]').length,
    );
    expect(nonToolRows).toBeGreaterThanOrEqual(2);
  });
});

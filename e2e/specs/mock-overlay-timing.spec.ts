/**
 * Mock SSE 回归测试：overlay 消费时序
 *
 * 使用 AG-UI（V2）协议格式 mock SSE，覆盖 PR #678 修复的两处缺陷：
 *   1. timeline_dispatch: `already_visible=true` 时延迟 finalize_loading_segment
 *   2. stream_end: `followup_pending=true` 时先 finalize_turn_projection 再 rotate
 *
 * 运行方式（前置：`cargo run -- serve` 在 127.0.0.1:8080 运行）：
 *   cd e2e && npm install && npx playwright test
 *
 * 注意：mock SSE 无法覆盖持久化验证（需 `conversation_saved` SSE 事件），
 *       持久化回归由 victauri_turn_layout.rs 覆盖。
 */

import { test, expect } from "@playwright/test";
import { seedSession, sendMessage, installMockSse } from "../fixtures/helpers";

const SID = "s_e2e_mock_overlay";

test.describe("overlay 消费时序回归", () => {
  // ---------------------------------------------------------------------------
  // 用例 1：final_response 后重复 assistant_answer_phase → 终答保留
  // ---------------------------------------------------------------------------
  test("zero_tool_final_response_then_second_answer_phase", async ({
    page,
  }) => {
    const sse = [
      // AG-UI: 正文相开始
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      // 纯文本 delta
      "id: 2\ndata: 我具备以下技能：文件读写、代码分析、命令执行与调试。\n\n",
      // AG-UI: timeline_log final_response
      'id: 3\ndata: {"type":"CUSTOM","customType":"timeline_log","data":{"kind":"","title":"final_response","detail":"我具备以下技能：文件读写、代码分析、命令执行与调试。"}}\n\n',
      // AG-UI: 第二次 assistant_answer_phase（触发 PendingFollowup）
      'id: 4\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      // AG-UI: 流结束 → on_done 处理
      'id: 5\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");

    await installMockSse(page, sse);
    await seedSession(page, SID + "_1");
    await sendMessage(page, "你有哪些技能");

    // 等待就绪 + 终答正文可见
    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText("我具备以下技能", { timeout: 5000 });

    // 无重复气泡：助手消息行数 ≤ 2（loading + final）
    const msgCount = await page.evaluate(
      () =>
        document.querySelectorAll('[data-testid="chat-message-row-assistant"]')
          .length,
    );
    expect(msgCount).toBeLessThanOrEqual(2);
  });

  // ---------------------------------------------------------------------------
  // 用例 2：长终答无截断
  // ---------------------------------------------------------------------------
  test("zero_tool_long_answer_not_truncated", async ({ page }) => {
    const answerHead =
      "这是一个较长的测试回复，用于验证终答内容在流式累积后完整保留不被截断。";
    const answerBody =
      "内容包括：文件读写能力、代码分析工具、命令执行环境、调试辅助功能、会话管理与持久化。";
    const answerTail =
      "以上功能均已通过 E2E 验证，确认终答气泡在无工具场景下正确显示完整内容。";

    const sse = [
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      `id: 2\ndata: ${answerHead}\n\n`,
      `id: 3\ndata: ${answerBody}\n\n`,
      `id: 4\ndata: ${answerTail}\n\n`,
      'id: 5\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");

    await installMockSse(page, sse);
    await seedSession(page, SID + "_2");
    await sendMessage(page, "详细介绍功能");

    // 等待就绪
    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );

    // 首部 + 尾部文本均可见（修复前：尾部可能被截断）
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(answerHead, { timeout: 5000 });
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(answerTail, { timeout: 5000 });

    // 纯问答不应有工具卡
    const toolCards = await page
      .locator('[data-testid="chat-tool-card"]')
      .count();
    expect(toolCards).toBe(0);
  });

  // ---------------------------------------------------------------------------
  // 用例 3：followup_pending 轮换保留旧终答
  // ---------------------------------------------------------------------------
  // 第二次 assistant_answer_phase 后直接 RUN_FINISHED（无 delta），
  // 此时 followup_pending 在 on_done 中处理。
  // 修复保证：先 finalize_turn_projection（从旧 overlay 创建 FINAL_ANSWER_ROW），再 rotate。
  // ---------------------------------------------------------------------------
  test("followup_pending_rotate_preserves_previous_answer", async ({
    page,
  }) => {
    const sse = [
      // 第一轮 text message
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      "id: 2\ndata: 文件读写功能已就绪。\n\n",
      // 第二轮 answer_phase（触发 PendingFollowup），无 delta →
      // followup_pending 保留到 on_done 中处理
      'id: 3\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      'id: 4\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");

    await installMockSse(page, sse);
    await seedSession(page, SID + "_3");
    await sendMessage(page, "检查功能状态");

    // 等待就绪 + 第一轮终答应可见
    // 修复前：followup_pending = on_done 中 rotate 先消费 overlay → FINAL_ANSWER_ROW 丢失
    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText("文件读写功能已就绪", { timeout: 5000 });
  });
});

/**
 * Mock SSE 回归测试：单流内气泡轮换不多产生空气泡
 *
 * 覆盖场景：后端在同一 SSE 流内发送第二轮 turn_segment_start(kind="answer")，
 * 验证旋转后不会因双重轮换产生多余的空气泡。
 *
 * 事件顺序模拟真实后端行为：
 *   assistant_answer_phase → delta(第一轮) → turn_segment_start(kind=answer)
 *   → assistant_answer_phase → delta(第二轮) → RUN_FINISHED
 *
 * 若存在双重轮换 bug，会导致第二轮产生 2 个气泡（1 个空 + 1 个有内容），
 * 本测试断言助手气泡数 = 2，且轮内容完整。
 */

import { test, expect } from "@playwright/test";
import { seedSession, sendMessage, installMockSse } from "../fixtures/helpers";

const TURN1_TEXT = "第一轮：分析项目结构。";
const TURN2_TEXT = "第二轮：生成构建脚本。";

test("单流内两轮气泡轮换不多产生空气泡", async ({ page }) => {
  // 模拟真实后端事件序列（同一 SSE 流内）
  const sse = [
    // 第一轮
    'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
    `id: 2\ndata: ${TURN1_TEXT}\n\n`,
    // 第二轮：turn_segment_start + assistant_answer_phase（顺序与后端一致）
    'id: 3\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-2","kind":"answer"}}\n\n',
    'id: 4\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
    `id: 5\ndata: ${TURN2_TEXT}\n\n`,
    // 流结束
    'id: 6\ndata: {"type":"RUN_FINISHED"}\n\n',
  ].join("");

  await installMockSse(page, sse);
  await seedSession(page, "s_e2e_stream_rotation_" + Date.now());
  await sendMessage(page, "分析并构造项目");

  // 等待流结束
  await expect(page.locator('[data-testid="status-bar"]')).toContainText(
    "就绪",
    { timeout: 25000 },
  );

  // 两轮的回答正文均可见
  const scroller = page.locator('[data-testid="chat-messages-scroller"]');
  await expect(scroller).toContainText(TURN1_TEXT, { timeout: 5000 });
  await expect(scroller).toContainText(TURN2_TEXT, { timeout: 5000 });

  // 核心断言：非工具消息行数恰好为 3（用户行 + 第一轮助手 + 第二轮助手）
  // 若存在双重轮换 bug，会产生第 4 行空气泡
  const nonToolRows = await page.evaluate(
    () => document.querySelectorAll('[data-testid="chat-message-row"]').length,
  );
  expect(nonToolRows).toBe(3);
});

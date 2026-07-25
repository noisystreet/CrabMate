import { expect, test } from "@playwright/test";
import {
  installDelayedMockSse,
  seedSession,
  sendMessage,
} from "../fixtures/helpers";

const SID = "e2e-tui-stream-view";

test("终端流按行渲染：流式半行纯文本，结束后 Markdown 生效", async ({
  page,
}) => {
  await seedSession(page, SID);
  await expect(page.getByTestId("chat-tui-stream-view")).toBeVisible();

  await installDelayedMockSse(
    page,
    [
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      `id: 2\ndata: ${JSON.stringify({
        type: "TEXT_MESSAGE_CONTENT",
        delta: "**第一段",
      })}\n\n`,
      `id: 3\ndata: ${JSON.stringify({
        type: "TEXT_MESSAGE_CONTENT",
        delta: "，第二段**",
      })}\n\n`,
      'id: 4\ndata: {"type":"RUN_FINISHED"}\n\n',
    ],
    220,
  );

  await sendMessage(page, "验证终端流");
  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(transcript).toContainText("用户");
  await expect(transcript).toContainText("验证终端流");
  await expect(transcript.locator(".chat-tui-turn--user")).toHaveCount(1);

  // 用户回合 section 在流式过程中应保持同一 DOM 节点（append-only）
  const userTurnStable = await page.evaluate(() => {
    const turn = document.querySelector(
      '.chat-tui-turn:not([data-tui-live="1"])',
    );
    if (!turn) return false;
    (turn as HTMLElement).dataset.probe = "stable-user";
    return true;
  });
  expect(userTurnStable).toBe(true);

  // 仅第一段到达时：半行保持字面量，尚未出现第二段；用户 section 未被整树重刷
  await expect(transcript).toContainText("**第一段");
  await expect(transcript).not.toContainText("第二段");
  await expect(transcript.locator("strong")).toHaveCount(0);
  await expect(
    page.locator('.chat-tui-turn[data-probe="stable-user"]'),
  ).toHaveCount(1);

  // 回合结束后 finalize：粗体生效；用户正文仍在（允许落定后因 id/水合变化重建 section）
  await expect(transcript.locator("strong")).toHaveCount(1, {
    timeout: 10_000,
  });
  await expect(transcript.locator("strong")).toHaveText("第一段，第二段");
  await expect(transcript).toContainText("验证终端流");
});

import { expect, test } from "@playwright/test";
import {
  installDelayedMockSse,
  seedSession,
  sendMessage,
} from "../fixtures/helpers";

const SID = "e2e-tui-stream-view";

test("默认终端流按纯文本增量展示且不解析 Markdown", async ({ page }) => {
  await seedSession(page, SID);
  await expect(page.getByTestId("chat-tui-stream-view")).toBeVisible();
  await expect(page.getByTestId("chat-view-mode-toggle")).toHaveCount(0);

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
    180,
  );

  await sendMessage(page, "验证终端流");
  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(transcript).toContainText("用户 ❯");
  await expect(transcript).toContainText("验证终端流");
  await expect(transcript).toContainText("**第一段");
  await expect(transcript.locator("strong")).toHaveCount(0);
  await expect(transcript).toContainText("**第一段，第二段**");
});

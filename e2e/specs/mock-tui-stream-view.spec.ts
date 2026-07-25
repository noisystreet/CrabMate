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
  await expect(transcript).toContainText("用户 ❯");
  await expect(transcript).toContainText("验证终端流");

  // 仅第一段到达时：半行保持字面量，尚未出现第二段
  await expect(transcript).toContainText("**第一段");
  await expect(transcript).not.toContainText("第二段");
  await expect(transcript.locator("strong")).toHaveCount(0);

  // 回合结束后 finalize：粗体生效
  await expect(transcript.locator("strong")).toHaveCount(1, {
    timeout: 10_000,
  });
  await expect(transcript.locator("strong")).toHaveText("第一段，第二段");
});

import { expect, test } from "@playwright/test";
import { installMockSse, seedSession, sendMessage } from "../fixtures/helpers";

const SID = "e2e-tui-tool-process";

test("终端流工具过程：一行摘要 + 可展开详情", async ({ page }) => {
  const postToolAnswer = "已读取文件内容。";
  const toolOutput = 'fn main() {\n    println!("Hello");\n}';
  const sse = [
    'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
    "id: 2\ndata: 我来读取文件。\n\n",
    'id: 3\ndata: {"type":"TOOL_CALL_START","toolCallId":"tc-tui-1","name":"read_file","summary":"读取成功"}\n\n',
    'id: 4\ndata: {"type":"CUSTOM","customType":"tool_running","data":{"running":true}}\n\n',
    `id: 5\ndata: ${JSON.stringify({
      type: "TOOL_CALL_RESULT",
      toolCallId: "tc-tui-1",
      content: toolOutput,
      metadata: { name: "read_file", ok: true, summary: "读取成功" },
    })}\n\n`,
    'id: 6\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
    `id: 7\ndata: ${JSON.stringify({
      type: "TEXT_MESSAGE_CONTENT",
      delta: postToolAnswer,
    })}\n\n`,
    'id: 8\ndata: {"type":"RUN_FINISHED"}\n\n',
  ].join("");

  await installMockSse(page, sse);
  await seedSession(page, SID);
  await sendMessage(page, "读取文件");

  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 25_000,
  });

  const process = page.getByTestId("chat-tui-tool-process");
  await expect(process).toHaveCount(1, { timeout: 10_000 });
  await expect(process).toContainText("read_file");
  await expect(process.locator(".chat-tui-tool-one-line")).toBeVisible();
  await expect(process.locator(".chat-tui-tool-row")).toBeVisible();
  // 折叠态固定单行高度，避免流式 ReplaceAll 抖高
  const rowHeight = await process
    .locator(".chat-tui-tool-row")
    .evaluate((el) => {
      const style = getComputedStyle(el);
      return { height: style.height, maxHeight: style.maxHeight };
    });
  expect(rowHeight.height).toBe(rowHeight.maxHeight);
  expect(parseFloat(rowHeight.height)).toBeGreaterThan(0);
  await expect(page.getByTestId("chat-tui-transcript")).toContainText(
    postToolAnswer,
  );

  const details = process.locator("details.chat-tui-tool-details");
  await expect(details).toHaveCount(1);
  await details.locator("summary").click();
  const detailBody = details.locator(".chat-tui-tool-detail-body");
  await expect(detailBody).toBeVisible();
  const detailText = await detailBody.innerText();
  expect(detailText.trim().length).toBeGreaterThan(0);
});

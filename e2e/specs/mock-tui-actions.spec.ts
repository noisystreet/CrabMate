import { expect, test } from "@playwright/test";
import {
  installDelayedMockSse,
  seedSession,
  sendMessage,
} from "../fixtures/helpers";

const SID = "e2e-tui-actions";

test("终端流：复制/重试挂在对应消息下方", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await seedSession(page, SID);
  await expect(page.getByTestId("chat-tui-turn-actions")).toHaveCount(0);

  await installDelayedMockSse(
    page,
    [
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      `id: 2\ndata: ${JSON.stringify({
        type: "TEXT_MESSAGE_CONTENT",
        delta: "可复制正文",
      })}\n\n`,
      'id: 3\ndata: {"type":"RUN_FINISHED"}\n\n',
    ],
    120,
  );

  await sendMessage(page, "触发操作条");
  await expect(page.getByTestId("chat-tui-transcript")).toContainText(
    "可复制正文",
    { timeout: 10_000 },
  );

  // 用户消息与助手消息各有一条下方操作条
  await expect(page.getByTestId("chat-tui-turn-actions")).toHaveCount(2, {
    timeout: 10_000,
  });

  const userWrap = page.locator(".chat-tui-turn-wrap--user").first();
  const userCopy = userWrap.getByTestId("chat-tui-action-copy");
  const userRegen = userWrap.getByTestId("chat-tui-action-regen");
  const userBranch = userWrap.getByTestId("chat-tui-action-branch");
  await expect(userCopy).toBeVisible();
  await expect(userRegen).toBeVisible();
  await expect(userBranch).toBeVisible();
  await expect(userCopy).toHaveClass(/msg-action-icon-btn/);
  await expect(userRegen).toHaveClass(/msg-action-icon-btn/);
  await expect(userBranch).toHaveClass(/msg-action-icon-btn/);
  await expect(userCopy.locator("svg.msg-action-icon")).toBeVisible();

  const assistantActions = page
    .locator(".chat-tui-turn-wrap:not(.chat-tui-turn-wrap--user)")
    .last()
    .getByTestId("chat-tui-turn-actions");
  const asstCopy = assistantActions.getByTestId("chat-tui-action-copy");
  await expect(asstCopy).toBeVisible();
  await expect(asstCopy).toHaveClass(/msg-action-icon-btn/);
  await expect(
    assistantActions.getByTestId("chat-tui-action-retry"),
  ).toHaveCount(0);

  await asstCopy.click();
  await expect
    .poll(async () => page.evaluate(() => navigator.clipboard.readText()), {
      timeout: 5_000,
    })
    .toContain("可复制正文");
});

test("终端流：失败助手消息下方可点重试", async ({ page }) => {
  await page.goto("/", { waitUntil: "networkidle", timeout: 20_000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15_000,
  });

  await page.evaluate((s: string) => {
    const body = JSON.stringify({
      sessions: [
        {
          id: s,
          title: "e2e-retry",
          draft: "",
          messages: [
            {
              id: "u-fail",
              role: "user",
              text: "会失败",
              reasoning_text: "",
              image_urls: [],
              state: null,
              is_tool: false,
              created_at: Date.now(),
            },
            {
              id: "a-fail",
              role: "assistant",
              text: "上一轮失败",
              reasoning_text: "",
              image_urls: [],
              state: "error",
              is_tool: false,
              created_at: Date.now(),
            },
          ],
          updated_at: Date.now(),
          pinned: false,
          starred: false,
        },
      ],
      active_session_id: s,
    });
    return fetch("/user-data/workspaces/current/sessions", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body,
    }).catch(() => {});
  }, `${SID}-retry`);

  await page.reload({ waitUntil: "networkidle", timeout: 20_000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15_000,
  });

  const retry = page
    .locator('.chat-tui-turn-wrap[data-tui-wrap-id="a-fail"]')
    .getByTestId("chat-tui-action-retry");
  await expect(retry).toBeVisible({ timeout: 10_000 });
  await expect(retry).toHaveClass(/msg-action-icon-btn/);
  await expect(retry.locator("svg.msg-action-icon")).toBeVisible();

  let streamHits = 0;
  await page.route("**/chat/stream", async (route) => {
    if (route.request().method() !== "POST") {
      await route.continue();
      return;
    }
    streamHits += 1;
    const sse = [
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      `id: 2\ndata: ${JSON.stringify({
        type: "TEXT_MESSAGE_CONTENT",
        delta: "重试成功",
      })}\n\n`,
      'id: 3\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");
    await route.fulfill({
      status: 200,
      headers: {
        "Content-Type": "text/event-stream",
        "x-conversation-id": "e2e-retry-conv",
        "x-stream-job-id": "e2e-retry-job",
      },
      body: sse,
    });
  });

  await retry.click();
  await expect(page.getByTestId("chat-tui-transcript")).toContainText(
    "重试成功",
    { timeout: 10_000 },
  );
  expect(streamHits).toBeGreaterThanOrEqual(1);
});

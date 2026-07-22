import { Page, Route } from "@playwright/test";

// ---------------------------------------------------------------------------
// 辅助函数：seed 会话 & prefs，mock SSE 拦截
// ---------------------------------------------------------------------------

/** 设置 prefs、session，重载页面等待输入框就绪。*/
export async function seedSession(page: Page, sid: string) {
  await page.goto("/", { waitUntil: "networkidle", timeout: 20000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15000,
  });

  await page.evaluate(() =>
    fetch("/user-data/prefs", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        locale: "zh",
        theme: "light",
        side_panel_view: "hidden",
        side_width: 280,
        editor_layout_mode: false,
      }),
    }).catch(() => {}),
  );

  await page.evaluate((s: string) => {
    const body = JSON.stringify({
      sessions: [
        {
          id: s,
          title: "e2e",
          draft: "",
          messages: [],
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
  }, sid);

  await page.reload({ waitUntil: "networkidle", timeout: 20000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15000,
  });
}

/** 在页面中发送消息（填值 + Enter）。*/
export async function sendMessage(page: Page, text: string) {
  await page.focus('[data-testid="chat-composer-input"]');
  await page.evaluate((msg: string) => {
    const el = document.querySelector<HTMLTextAreaElement>(
      '[data-testid="chat-composer-input"]',
    );
    if (!el) return;
    const s = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!;
    s.call(el, msg);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }, text);
  await page.keyboard.press("Enter");
}

/** 注册 page.route 拦截 /chat/stream POST -> 返回 mock SSE 正文。
 * 必须携带 x-conversation-id / x-stream-job-id 头，前端据此初始化流式会话。 */
export function installMockSse(
  page: Page,
  sseBody: string,
  convId = "e2e-conv",
) {
  return page.route("**/chat/stream", (route) => {
    if (route.request().method() !== "POST") {
      return route.continue();
    }
    return route.fulfill({
      status: 200,
      headers: {
        "content-type": "text/event-stream; charset=utf-8",
        "x-conversation-id": convId,
        "x-stream-job-id": "1",
      },
      body: sseBody,
    });
  });
}

/** 等待页面中出现指定文本（超时 ms）。*/
export async function waitForText(page: Page, text: string, timeoutMs = 20000) {
  await page.waitForFunction((t) => document.body.innerText.includes(t), text, {
    timeout: timeoutMs,
  });
}

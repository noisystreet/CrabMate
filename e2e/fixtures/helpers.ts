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

// ---------------------------------------------------------------------------
// 真实 LLM 测试辅助函数
// ---------------------------------------------------------------------------

/** 健康检查：poll /health 直到后端就绪。*/
export async function waitForHealth(
  baseUrl: string,
  maxRetries = 15,
  intervalMs = 1000,
) {
  for (let i = 0; i < maxRetries; i++) {
    try {
      const resp = await fetch(`${baseUrl}/health`);
      if (resp.ok) return;
    } catch {
      // 后端尚未就绪
    }
    await new Promise((r) => setTimeout(r, intervalMs));
  }
  throw new Error(`后端 ${baseUrl} 在 ${maxRetries * intervalMs}ms 内未就绪`);
}

/** 为真实 LLM 测试设置会话：API key + LLM 配置 + prefs + 空会话。
 *
 * 流程：
 *   1. 导航至首页并等待输入框就绪
 *   2. 设置 client_llm.api_key 到后端 secrets 存储
 *   3. 设置 LLM 覆盖配置（api_base、model 等）
 *   4. 设置用户偏好
 *   5. 创建空会话
 *   6. 重载页面等待 UI 就绪
 */
export async function setupRealLLMSession(
  page: Page,
  sid: string,
  apiKey: string,
  llmConfig?: {
    apiBase?: string;
    model?: string;
    contextTokens?: string;
    thinkingMode?: string;
  },
) {
  const cfg = {
    apiBase: llmConfig?.apiBase ?? "https://api.deepseek.com",
    model: llmConfig?.model ?? "deepseek-chat",
    contextTokens: llmConfig?.contextTokens ?? "1000000",
    thinkingMode: llmConfig?.thinkingMode ?? "off",
  };

  await page.goto("/", { waitUntil: "networkidle", timeout: 20000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15000,
  });

  // 设置 API key
  await page.evaluate(
    (key: string) =>
      fetch("/user-data/secrets/client-llm", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ api_key: key }),
      }),
    apiKey,
  );

  // 设置 LLM 覆盖配置
  await page.evaluate(
    (c: {
      apiBase: string;
      model: string;
      contextTokens: string;
      thinkingMode: string;
    }) =>
      fetch("/user-data/llm-overrides", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          client_llm: {
            api_base: c.apiBase,
            model: c.model,
            llm_context_tokens: c.contextTokens,
            llm_thinking_mode: c.thinkingMode,
          },
        }),
      }),
    cfg,
  );

  // 设置 prefs
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

  // 创建空会话
  await page.evaluate((s: string) => {
    const body = JSON.stringify({
      sessions: [
        {
          id: s,
          title: "e2e-real-llm",
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

  // 重载等待 UI
  await page.reload({ waitUntil: "networkidle", timeout: 20000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15000,
  });
}

/** 等待状态栏出现「就绪」文本（真实 LLM 调用可能耗时较长）。*/
export async function waitForReady(page: Page, timeout = 120_000) {
  await page.waitForFunction(() => document.body.innerText.includes("就绪"), {
    timeout,
  });
}

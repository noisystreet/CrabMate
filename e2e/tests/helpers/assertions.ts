import { expect, type Page } from '@playwright/test';

import { fillComposerDraft } from './composer';
import { PAGINATE_PAGE_LIMIT } from './seed-conversation';

/** 大多数 stub 流式 UI 断言超时（ms）。与 playwright.config.ts 的 actionTimeout 20s 对齐。 */
export const UI_TIMEOUT = 20_000;

/** 滚动到位断言超时（ms）。setTimeout + rAF 双帧沉降约需 2-3 帧，足够此窗口。 */
export const SCROLL_TIMEOUT = 5_000;

/** 当前可见的对话层（与 IDE 层同挂载时避免匹配到 `visibility:hidden` 的副本）。 */
export function visibleChatLayer(page: import('@playwright/test').Page) {
  return page.locator('.main-row-chat-layer:not(.main-row-chat-layer--hidden)');
}

/** 等待尾部页水合完成（`limit` + `has_older`，避免误匹配其它 GET）。 */
export function waitForConversationMessages(
  page: Page,
  conversationId: string,
): Promise<import('@playwright/test').Response> {
  const enc = encodeURIComponent(conversationId);
  return page.waitForResponse(
    async (res) => {
      if (!res.ok() || res.request().method() !== 'GET') {
        return false;
      }
      const url = res.url();
      if (!url.includes('/conversation/messages') || !url.includes(`conversation_id=${enc}`)) {
        return false;
      }
      if (!url.includes(`limit=${PAGINATE_PAGE_LIMIT}`)) {
        return false;
      }
      try {
        const body = (await res.json()) as { has_older?: boolean; messages?: unknown[] };
        return body.has_older === true && (body.messages?.length ?? 0) === PAGINATE_PAGE_LIMIT;
      } catch {
        return false;
      }
    },
    { timeout: UI_TIMEOUT },
  );
}

/** 发送消息并等待 POST /chat/stream 响应结束。 */
export async function sendStubMessage(page: Page, text: string): Promise<void> {
  await expect(page.getByTestId('chat-composer-input')).toBeVisible({ timeout: UI_TIMEOUT });
  await expect(page.getByTestId('chat-send-button')).toBeEnabled({ timeout: UI_TIMEOUT });
  const streamDone = page.waitForResponse(
    (res) => res.url().includes('/chat/stream') && res.request().method() === 'POST',
    { timeout: UI_TIMEOUT },
  );
  await fillComposerDraft(page, text);
  await page.getByTestId('chat-send-button').click();
  await streamDone;
}

/**
 * 等待流式完成：响应结束 → 发送按钮恢复 → 停止按钮消失 → loading 状态清除。
 * 覆盖 `waitForResponse` 之后浏览器仍需处理 SSE 事件和 DOM 更新的时间窗口。
 */
export async function waitForStreamComplete(page: Page): Promise<void> {
  const streamDone = page.waitForResponse(
    (res) => res.url().includes('/chat/stream') && res.request().method() === 'POST',
    { timeout: UI_TIMEOUT },
  );
  await streamDone;

  // 等 UI 恢复就绪
  await expect(page.getByTestId('chat-send-button')).toBeEnabled({ timeout: UI_TIMEOUT });
  await expect(page.getByRole('button', { name: '停止' })).toBeDisabled({ timeout: UI_TIMEOUT });

  // 等所有消息去掉 loading 类名（流式尾泡收尾）
  await expect(page.locator('.msg-loading')).toHaveCount(0, { timeout: UI_TIMEOUT });

  // 再等一帧确保 rAF DOM 沉降完成
  await page.waitForTimeout(50);
}

/** 助手正文（非工具卡）可见。 */
export async function expectAssistantText(page: Page, text: string | RegExp): Promise<void> {
  await expect(
    page.getByTestId('chat-message-row').filter({ hasText: text }),
  ).toBeVisible({ timeout: UI_TIMEOUT });
}

export async function expectToolCardVisible(page: Page): Promise<void> {
  await expect(page.getByTestId('chat-tool-card').first()).toBeVisible({ timeout: UI_TIMEOUT });
}

import { expect, type Page } from '@playwright/test';

import { fillComposerDraft } from './composer';
import { PAGINATE_PAGE_LIMIT } from './seed-conversation';

export const UI_TIMEOUT = 45_000;

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

/** 助手正文（非工具卡）可见。 */
export async function expectAssistantText(page: Page, text: string | RegExp): Promise<void> {
  await expect(
    page.getByTestId('chat-message-row').filter({ hasText: text }),
  ).toBeVisible({ timeout: UI_TIMEOUT });
}

export async function expectToolCardVisible(page: Page): Promise<void> {
  await expect(page.getByTestId('chat-tool-card').first()).toBeVisible({ timeout: UI_TIMEOUT });
}

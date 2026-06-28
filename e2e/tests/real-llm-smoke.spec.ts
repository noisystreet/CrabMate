import { expect, test } from '@playwright/test';

import { fillComposerDraft, putFreshLocalSession, UI_TIMEOUT } from './helpers';

const REAL_LLM_TIMEOUT = 180_000;
const REAL_LLM_ENABLED = process.env.REAL_LLM_E2E === '1';

test.describe('real LLM smoke', () => {
  test.skip(!REAL_LLM_ENABLED, 'set REAL_LLM_E2E=1 to run against the configured model backend');

  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_real_llm', 'E2E real LLM');
  });

  test('skills prompt streams a real model reply and releases busy state', async ({ page }) => {
    test.setTimeout(REAL_LLM_TIMEOUT + 30_000);

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible({
      timeout: UI_TIMEOUT,
    });

    const streamDone = page.waitForResponse(
      (res) => res.url().includes('/chat/stream') && res.request().method() === 'POST',
      { timeout: REAL_LLM_TIMEOUT },
    );
    await fillComposerDraft(page, '你有哪些技能');
    await page.getByTestId('chat-send-button').click();
    const response = await streamDone;
    expect(response.ok(), await response.text()).toBeTruthy();

    await expect(page.getByTestId('chat-send-button')).toBeEnabled({ timeout: UI_TIMEOUT });
    await expect(page.getByRole('button', { name: '停止' })).toBeDisabled({ timeout: UI_TIMEOUT });
    await expect(page.getByTestId('chat-message-row').filter({ hasText: /助手/ }).last()).toBeVisible({
      timeout: UI_TIMEOUT,
    });
    await expect(page.getByTestId('chat-messages-scroller')).not.toContainText(/对话失败|请求失败/);
  });
});

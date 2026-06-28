import { expect, test } from '@playwright/test';

import {
  UI_TIMEOUT,
  fillComposerDraft,
  installTwoTurnChatStreamStub,
  putFreshLocalSession,
} from './helpers';

test.describe('two-turn without hydrate', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_two_turn_no_h', 'E2E no-hydrate');
  });

  test('fast second turn completes', async ({ page }) => {
    await installTwoTurnChatStreamStub(page);
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await fillComposerDraft(page, '你好');
    await page.getByTestId('chat-send-button').click();
    await expect(page.getByTestId('chat-message-row').filter({ hasText: /CrabMate 助手/ })).toBeVisible({
      timeout: UI_TIMEOUT,
    });

    await fillComposerDraft(page, '你有哪些技能');
    await page.getByTestId('chat-send-button').click();
    await expect(page.getByTestId('chat-message-row').filter({ hasText: /E2E stub 技能列表/ })).toBeVisible({
      timeout: UI_TIMEOUT,
    });
  });

  test('slow second turn without hydrate completes', async ({ page }) => {
    test.setTimeout(120_000);
    await installTwoTurnChatStreamStub(page, { slowSecondTurnMs: 80 });
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await fillComposerDraft(page, '你好');
    await page.getByTestId('chat-send-button').click();
    await expect(page.getByTestId('chat-message-row').filter({ hasText: /CrabMate 助手/ })).toBeVisible({
      timeout: UI_TIMEOUT,
    });

    const stream2 = page.waitForResponse(
      (res) => res.url().includes('/chat/stream') && res.request().method() === 'POST',
      { timeout: 90_000 },
    );
    await fillComposerDraft(page, '你有哪些技能');
    await page.getByTestId('chat-send-button').click();
    await stream2;

    await expect(page.getByTestId('chat-message-row').filter({ hasText: /E2E stub 技能列表/ })).toBeVisible({
      timeout: UI_TIMEOUT,
    });
  });
});

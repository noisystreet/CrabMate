import { expect, test } from '@playwright/test';

import {
  UI_TIMEOUT,
  fillComposerDraft,
  installTwoTurnChatStreamStub,
  putFreshLocalSession,
} from './helpers';

test.describe('two-turn slow stream', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_two_turn_dbg', 'E2E debug');
  });

  test('slow second turn completes and releases busy state', async ({ page }) => {
    test.setTimeout(30_000);
    await installTwoTurnChatStreamStub(page, { slowSecondTurnMs: 80 });
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await fillComposerDraft(page, '你好');
    await page.getByTestId('chat-send-button').click();
    await expect(page.getByTestId('chat-message-row').filter({ hasText: /CrabMate 助手/ })).toBeVisible({
      timeout: UI_TIMEOUT,
    });

    await fillComposerDraft(page, '你有哪些技能');
    await page.getByTestId('chat-send-button').click();

    await expect
      .poll(
        async () => {
          const busy = await page.locator('footer').getByText('模型生成中').count();
          const skill = await page.getByTestId('chat-message-row').filter({ hasText: /E2E stub 技能列表/ }).count();
          return skill > 0 ? 'done' : busy > 0 ? 'busy' : 'idle';
        },
        { timeout: 15_000, intervals: [200] },
      )
      .toBe('done');
  });
});

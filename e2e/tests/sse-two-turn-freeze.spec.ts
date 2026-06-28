import { expect, test } from '@playwright/test';

import {
  UI_TIMEOUT,
  fillComposerDraft,
  installTwoTurnChatStreamStub,
  installTwoTurnHydrateStub,
  putFreshLocalSession,
} from './helpers';

async function skillsAnswerText(page: import('@playwright/test').Page): Promise<string> {
  const row = page
    .getByTestId('chat-message-row')
    .filter({ hasText: /我可以帮你|白名单|E2E stub 技能/ })
    .last();
  if ((await row.count()) === 0) {
    return '';
  }
  const answer = row.locator('.msg-md-answer').first();
  if ((await answer.count()) === 0) {
    return (await row.innerText()).trim();
  }
  return (await answer.innerText()).trim();
}

test.describe('two-turn greeting then skills (stub SSE)', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_two_turn', 'E2E two-turn');
  });

  test('after hydrate, slow SSE second turn still completes', async ({ page }) => {
    test.setTimeout(120_000);
    await installTwoTurnChatStreamStub(page, { slowSecondTurnMs: 80 });
    await installTwoTurnHydrateStub(page);

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    const hydrateAfterTurn1 = page.waitForResponse(
      (res) =>
        res.url().includes('/conversation/messages') &&
        res.request().method() === 'GET' &&
        res.ok(),
      { timeout: UI_TIMEOUT },
    );

    await fillComposerDraft(page, '你好');
    await page.getByTestId('chat-send-button').click();
    await expect(page.getByTestId('chat-message-row').filter({ hasText: /CrabMate 助手/ })).toBeVisible({
      timeout: UI_TIMEOUT,
    });
    await hydrateAfterTurn1;

    const stream2 = page.waitForResponse(
      (res) => res.url().includes('/chat/stream') && res.request().method() === 'POST',
      { timeout: UI_TIMEOUT },
    );
    await fillComposerDraft(page, '你有哪些技能');
    await page.getByTestId('chat-send-button').click();
    await stream2;

    await expect(page.getByTestId('chat-message-row').filter({ hasText: /E2E stub 技能列表/ })).toBeVisible({
      timeout: UI_TIMEOUT,
    });
    await expect(page.locator('.status-bar')).not.toContainText(/模型生成中|Generating/i, {
      timeout: UI_TIMEOUT,
    });

    const body = await skillsAnswerText(page);
    expect(body).toContain('我可以帮你');
    expect(body).toContain('E2E stub 技能列表');
  });
});

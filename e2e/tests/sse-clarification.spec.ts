import { expect, test } from '@playwright/test';

import {
  buildClarificationStreamBody,
  buildDefaultStreamBody,
  expectAssistantText,
  fillClarificationAnswer,
  installChatStreamStub,
  putFreshLocalSession,
  sendStubMessage,
  UI_TIMEOUT,
} from './helpers';

test.describe('SSE clarification questionnaire', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_clarify');
  });

  test('questionnaire shows panel, submit triggers second stream', async ({ page }) => {
    await installChatStreamStub(page, {
      bodies: [
        buildClarificationStreamBody(),
        buildDefaultStreamBody({ assistantDelta: 'E2E after clarify.' }),
      ],
    });

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();
    await expect(page.getByTestId('chat-composer-input')).toBeVisible({ timeout: UI_TIMEOUT });

    await sendStubMessage(page, 'e2e clarify');
    const panel = page.getByTestId('composer-clarification-panel');
    await expect(panel).toBeVisible();
    await expect(panel).toContainText('E2E please clarify');
    await expect(panel).toContainText('Scope?');

    await fillClarificationAnswer(page, 0, 'backend only');
    const secondStream = page.waitForResponse(
      (res) => res.url().includes('/chat/stream') && res.request().method() === 'POST',
      { timeout: UI_TIMEOUT },
    );
    await page.getByTestId('composer-clarification-submit').click();
    await secondStream;

    await expect(panel).not.toBeVisible();
    await expectAssistantText(page, 'E2E after clarify.');
  });
});

import { expect, test } from '@playwright/test';

import {
  REAL_LLM_ENABLED,
  REAL_LLM_TIMEOUT,
  gotoCrabMateHome,
  putFreshLocalSession,
  sendAndWaitForStream,
} from './helpers';

test.describe('real LLM smoke', () => {
  test.skip(!REAL_LLM_ENABLED, 'set REAL_LLM_E2E=1 to run against the configured model backend');

  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_real_llm', 'E2E real LLM');
  });

  test('skills prompt streams a real model reply and releases busy state', async ({ page }) => {
    test.setTimeout(REAL_LLM_TIMEOUT + 30_000);

    await gotoCrabMateHome(page);
    await sendAndWaitForStream(page, '你有哪些技能');

    await expect(page.getByTestId('chat-message-row').filter({ hasText: /助手/ }).last()).toBeVisible({
      timeout: REAL_LLM_TIMEOUT,
    });
    await expect(page.getByTestId('chat-messages-scroller')).not.toContainText(/对话失败|请求失败/);
  });
});

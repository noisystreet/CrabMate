import { expect, test } from '@playwright/test';

import {
  installChatApprovalStub,
  installChatStreamStub,
  putFreshLocalSession,
  sendStubMessage,
} from './helpers';

test.describe('SSE approval modal actions', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_appr_act');
  });

  test('deny closes modal without chat failed banner', async ({ page }) => {
    await installChatStreamStub(page, { preset: 'command_approval' });
    await installChatApprovalStub(page);
    await page.goto('/');
    await sendStubMessage(page, 'e2e approval deny');

    const modal = page.getByTestId('approval-modal');
    await expect(modal).toBeVisible();
    await page.getByTestId('approval-deny').click();
    await expect(modal).not.toBeVisible();
    await expect(page.locator('.status-bar')).not.toContainText(/对话失败|Chat failed/i);
  });

  test('allow once closes modal', async ({ page }) => {
    await installChatStreamStub(page, { preset: 'command_approval' });
    await installChatApprovalStub(page);
    await page.goto('/');
    await sendStubMessage(page, 'e2e approval allow');

    const modal = page.getByTestId('approval-modal');
    await expect(modal).toBeVisible();
    await page.getByTestId('approval-allow-once').click();
    await expect(modal).not.toBeVisible();
  });
});

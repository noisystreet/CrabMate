import { expect, test } from '@playwright/test';

import {
  installChatApprovalStub,
  installChatStreamStub,
  putFreshLocalSession,
  sendStubMessage,
} from './helpers';

test.describe('SSE control plane (stub stream)', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_sse');
  });

  test('stream error with code shows chat failed in status bar', async ({ page }) => {
    await installChatStreamStub(page, { preset: 'stream_error' });
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await sendStubMessage(page, 'e2e error');

    await expect(page.locator('.status-bar')).toContainText(/对话失败|Chat failed/i);
  });

  test('command_approval_request opens approval modal with command preview', async ({ page }) => {
    await installChatStreamStub(page, { preset: 'command_approval' });
    await installChatApprovalStub(page);
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await sendStubMessage(page, 'e2e approval');

    const modal = page.getByTestId('approval-modal');
    await expect(modal).toBeVisible();
    await expect(modal).toContainText(/命令审批|Command Approval/i);
    await expect(modal.locator('.approval-modal-command')).toContainText('git status');

    await modal.getByRole('button', { name: /拒绝|Deny/i }).click();
    await expect(modal).not.toBeVisible();
  });
});

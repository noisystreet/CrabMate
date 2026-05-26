import { expect, test } from '@playwright/test';

import { openSessionListModal, putWorkspaceSessions } from './helpers';

test.describe('session CRUD (UI)', () => {
  test.beforeEach(async ({ request }) => {
    await putWorkspaceSessions(
      request,
      [
        {
          id: 's_e2e_keep',
          title: 'E2E keep',
          draft: '',
          messages: [{ id: 'm1', role: 'user', text: 'keep me' }],
          updated_at: 2,
          pinned: false,
          starred: false,
        },
        {
          id: 's_e2e_drop',
          title: 'E2E drop',
          draft: '',
          messages: [{ id: 'm2', role: 'user', text: 'drop me' }],
          updated_at: 1,
          pinned: false,
          starred: false,
        },
      ],
      's_e2e_keep',
    );
  });

  test('new chat creates another session in rail', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('nav-session-s_e2e_keep')).toBeVisible();
    await page.getByTestId('nav-new-chat').click();
    await expect(page.getByText(/新会话|New chat/i)).toBeVisible();
    const sessions = page.locator('[data-testid^="nav-session-"]');
    await expect(sessions).toHaveCount(3);
  });

  test('pin session in manage modal shows pin badge in rail', async ({ page }) => {
    await page.goto('/');
    await openSessionListModal(page);
    await page.getByTestId('session-modal-pin-s_e2e_drop').click();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('session-list-modal')).not.toBeVisible();

    const railItem = page.getByTestId('nav-session-s_e2e_drop');
    await expect(railItem).toHaveClass(/is-pinned/);
  });

  test('delete session after confirm removes it from modal list', async ({ page }) => {
    await page.goto('/');
    await openSessionListModal(page);
    page.once('dialog', (dialog) => dialog.accept());
    await page.getByTestId('session-modal-delete-s_e2e_drop').click();
    await expect(page.getByTestId('session-modal-open-s_e2e_drop')).not.toBeVisible();
    await expect(page.getByTestId('nav-session-s_e2e_drop')).not.toBeVisible();
  });
});

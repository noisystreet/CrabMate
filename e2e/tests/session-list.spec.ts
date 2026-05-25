import { expect, test } from '@playwright/test';

import { openSessionListModal, putWorkspaceSessions } from './helpers';

test.describe('session list modal', () => {
  test.beforeEach(async ({ request }) => {
    await putWorkspaceSessions(
      request,
      [
        {
          id: 's_e2e_a',
          title: 'E2E session A',
          draft: '',
          messages: [{ id: 'm_e2e_a', role: 'user', text: 'hello a' }],
          updated_at: 2,
          pinned: false,
          starred: false,
        },
        {
          id: 's_e2e_b',
          title: 'E2E session B',
          draft: '',
          messages: [{ id: 'm_e2e_b', role: 'user', text: 'hello b' }],
          updated_at: 1,
          pinned: false,
          starred: false,
        },
      ],
      's_e2e_a',
    );
  });

  test('manage sessions modal switches active session', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();
    await expect(page.getByText('hello a')).toBeVisible();

    await openSessionListModal(page);
    await page.getByTestId('session-modal-open-s_e2e_b').click();
    await expect(page.getByTestId('session-list-modal')).not.toBeVisible();

    await expect(page.getByText('hello b')).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText('hello a')).not.toBeVisible();
  });
});

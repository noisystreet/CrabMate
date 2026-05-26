import { expect, test } from '@playwright/test';

import { putFreshLocalSession, putUserPrefs } from './helpers';

test.describe('user prefs side panel', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_side');
  });

  test('workspace side panel opens on load when prefs say workspace', async ({ page, request }) => {
    await putUserPrefs(request, { side_panel_view: 'workspace', side_width: 320 });

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();
    await expect(page.getByTestId('workspace-panel')).toBeVisible({ timeout: 15_000 });
  });
});

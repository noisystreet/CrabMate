import { expect, test } from '@playwright/test';

import { putFreshLocalSession, putUserPrefs } from './helpers';

test.describe('user prefs theme hydration', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_theme');
  });

  test('dark theme from GET /user-data/prefs applies data-theme on load', async ({ page, request }) => {
    await putUserPrefs(request, { theme: 'dark', side_panel_view: 'hidden' });

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark', { timeout: 15_000 });
  });
});

import { expect, test } from '@playwright/test';

import {
  closeSettingsPage,
  openSettingsPage,
  openSettingsSection,
  putFreshLocalSession,
  putUserPrefs,
  saveSettingsPage,
  UI_TIMEOUT,
} from './helpers';

test.describe('settings page appearance', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_settings_app');
    await putUserPrefs(request, { theme: 'light', side_panel_view: 'hidden' });
  });

  test('theme change saves to user-data prefs', async ({ page, request }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await openSettingsPage(page);
    await openSettingsSection(page, 'appearance');
    await page.locator('#settings-page-appearance-theme').selectOption('dark');

    const prefsPut = page.waitForResponse(
      (res) =>
        res.url().includes('/user-data/prefs') &&
        res.request().method() === 'PUT' &&
        res.ok(),
      { timeout: UI_TIMEOUT },
    );
    await saveSettingsPage(page);
    await prefsPut;
    await closeSettingsPage(page);

    await expect
      .poll(async () => {
        const prefs = await request.get('/user-data/prefs');
        const body = (await prefs.json()) as { theme: string };
        return body.theme;
      })
      .toBe('dark');

    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  });
});

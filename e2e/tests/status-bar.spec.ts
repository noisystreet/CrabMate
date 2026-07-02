import { expect, test } from '@playwright/test';

import { putFreshLocalSession } from './helpers';

test.describe('status bar', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_status');
  });

  test('status fetch error shows fetch-error styling on footer', async ({ page }) => {
    await page.route('**/status', async (route) => {
      if (route.request().method() === 'GET') {
        await route.fulfill({
          status: 503,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'e2e status unavailable' }),
        });
        return;
      }
      await route.continue();
    });

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    const bar = page.getByTestId('status-bar');
    await expect(bar).toBeVisible();
    await expect(bar).toHaveClass(/status-bar-fetch-error/);
  });

  test('footer status bar is present on load', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('status-bar')).toBeVisible();
  });

  test('agent role trigger opens upward menu', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('status-bar')).toBeVisible();
    const trigger = page.getByTestId('status-agent-role-trigger');
    await expect(trigger).toBeVisible();
    await trigger.click();
    const menu = page.locator('.status-agent-role-menu');
    await expect(menu).toBeVisible();
    await expect(menu.getByRole('menuitem').first()).toBeVisible();
  });
});

import { expect, test } from '@playwright/test';

test.describe('user prefs theme hydration', () => {
  test('dark theme from GET /user-data/prefs applies data-theme on load', async ({ page, request }) => {
    const put = await request.put('/user-data/prefs', {
      data: {
        locale: 'zh',
        theme: 'dark',
        side_panel_view: 'hidden',
        side_width: 280,
      },
    });
    expect(put.status()).toBe(204);

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark', { timeout: 15_000 });
  });
});

import { expect, test } from '@playwright/test';
import { installChatStreamStub } from './fix-chat-stream';

test.describe('Web UI smoke', () => {
  test('send message shows assistant reply and tool card (stub stream)', async ({ page }) => {
    await installChatStreamStub(page);

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    const input = page.getByTestId('chat-composer-input');
    await input.fill('e2e ping');
    await page.getByTestId('chat-send-button').click();

    await expect(page.locator('.msg-assistant .msg-body').filter({ hasText: 'Hello from E2E stub' })).toBeVisible({
      timeout: 30_000,
    });
    await expect(page.getByTestId('chat-tool-card').first()).toBeVisible();
  });

  test('workspace panel shows stubbed tree', async ({ page }) => {
    await page.route('**/workspace', async (route) => {
      const method = route.request().method();
      if (method === 'POST') {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ ok: true, path: '/e2e-mock-root' }),
        });
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          path: '/e2e-mock-root',
          entries: [{ name: 'e2e-stub.txt', is_dir: false }],
          error: null,
        }),
      });
    });

    await page.goto('/');
    await page.getByTestId('side-view-trigger').click();
    await page.getByTestId('side-panel-workspace-menu').click();

    const panel = page.getByTestId('workspace-panel');
    await expect(panel).toBeVisible();
    await expect(panel.getByTestId('workspace-root-input')).toBeVisible();
    await expect(panel.getByTestId('workspace-file-tree')).toContainText('e2e-stub.txt', {
      timeout: 15_000,
    });
  });
});

import { expect, test } from '@playwright/test';

import { putFreshLocalSession } from './helpers';

const STUB_FILE = 'e2e-ide-stub.txt';
let fileContent = 'hello ide';

test.describe('IDE layout', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_ide');
    fileContent = 'hello ide';
  });

  test('switch to editor, open file, edit, save, return to chat', async ({ page }) => {
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
          entries: [{ name: STUB_FILE, is_dir: false }],
          error: null,
        }),
      });
    });

    await page.route('**/workspace/file**', async (route) => {
      const method = route.request().method();
      const url = new URL(route.request().url());
      if (method === 'GET') {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            path: STUB_FILE,
            content: fileContent,
            error: null,
          }),
        });
        return;
      }
      if (method === 'POST') {
        const body = route.request().postDataJSON() as { content?: string };
        if (typeof body.content === 'string') {
          fileContent = body.content;
        }
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ error: null }),
        });
        return;
      }
      await route.fulfill({ status: 405, body: 'method not allowed' });
    });

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await page.getByTestId('layout-mode-toggle').first().click();
    const ide = page.getByTestId('ide-layout-root');
    await expect(ide).toBeVisible();

    const ideTree = ide.getByTestId('workspace-file-tree');
    await expect(ideTree).toContainText(STUB_FILE, {
      timeout: 15_000,
    });
    await ideTree.getByText(STUB_FILE).click();

    const editor = page.getByTestId('ide-editor-textarea');
    await expect(editor).toBeEnabled({ timeout: 10_000 });
    await expect(editor).toHaveValue('hello ide');

    await editor.fill('hello ide e2e');
    await page.keyboard.press('Control+S');

    await expect.poll(async () => fileContent).toBe('hello ide e2e');

    await ide.getByTestId('layout-mode-toggle').click();
    await expect(ide).toBeHidden();
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();
  });
});

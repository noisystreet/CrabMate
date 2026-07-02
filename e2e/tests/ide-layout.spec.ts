import { expect, test, type Page } from '@playwright/test';

import { ensureChatLayoutPrefs, putFreshLocalSession } from './helpers';

const STUB_FILE = 'e2e-ide-stub.txt';
const CHAT_PRELOAD_DWELL_MS = 600;

let fileContent = 'hello ide';

function stubIdeWorkspaceRoutes(page: Page): void {
  void page.route('**/workspace', async (route) => {
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

  void page.route('**/workspace/file**', async (route) => {
    const method = route.request().method();
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
}

/** 对话模式：IDE 叠层已挂载但隐藏（回归 visibility:hidden 下预创建 CodeMirror）。 */
async function expectChatModeWithHiddenIdeLayer(page: Page): Promise<void> {
  await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();
  await expect(page.getByTestId('ide-layout-root')).toBeHidden();
  await expect(page.locator('.main-row-ide-layer')).toHaveClass(/main-row-ide-layer--hidden/);
  // 首屏对话模式：须在隐藏层预创建 CM（旧 bug）；进入 IDE 后再切回对话可保留实例。
  await expect(page.locator('[data-testid="ide-editor-cm"] .cm-editor')).toHaveCount(0);
  await page.waitForTimeout(CHAT_PRELOAD_DWELL_MS);
}

async function ensureNoIdeConfirmBlocking(page: Page): Promise<void> {
  const dialog = page.getByTestId('ide-confirm-dialog');
  if (await dialog.isVisible()) {
    await page.getByTestId('ide-confirm-cancel').click();
    await expect(dialog).toBeHidden();
  }
}

async function switchToIdeFromChat(page: Page): Promise<ReturnType<Page['locator']>> {
  await page.getByTestId('layout-mode-toggle').first().click();
  const ide = page.getByTestId('ide-layout-root');
  await expect(ide).toBeVisible();
  await ensureNoIdeConfirmBlocking(page);
  return ide;
}

async function switchToChatFromIde(page: Page): Promise<void> {
  const ide = page.getByTestId('ide-layout-root');
  await page.getByTestId('layout-mode-toggle').click();
  await expect(ide).toBeHidden();
  await ensureNoIdeConfirmBlocking(page);
}

async function switchToIdeAndOpenStubFile(page: Page): Promise<ReturnType<Page['locator']>> {
  const ide = await switchToIdeFromChat(page);

  const ideTree = ide.getByTestId('workspace-file-tree');
  await expect(ideTree).toContainText(STUB_FILE, { timeout: 15_000 });
  await ideTree.getByText(STUB_FILE).click();

  const editorHost = ide.getByTestId('ide-editor-cm');
  const cmContent = editorHost.locator('.cm-content');
  await expect(cmContent).toBeVisible({ timeout: 10_000 });
  return cmContent;
}

test.describe('IDE layout', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_ide');
    fileContent = 'hello ide';
  });

  test.afterEach(async ({ request }) => {
    await ensureChatLayoutPrefs(request);
  });

  test('chat preload then switch to IDE shows editor content', async ({ page }) => {
    stubIdeWorkspaceRoutes(page);

    await page.goto('/');
    await expectChatModeWithHiddenIdeLayer(page);

    const cmContent = await switchToIdeAndOpenStubFile(page);
    await expect(cmContent).toContainText('hello ide');
  });

  test('switch to editor, open file, edit, save, return to chat', async ({ page }) => {
    stubIdeWorkspaceRoutes(page);

    await page.goto('/');
    await expectChatModeWithHiddenIdeLayer(page);

    const cmContent = await switchToIdeAndOpenStubFile(page);
    await expect(cmContent).toContainText('hello ide');

    await cmContent.click();
    await page.keyboard.press('Control+a');
    await page.keyboard.type('hello ide e2e');
    await page.keyboard.press('Control+S');

    await expect.poll(async () => fileContent).toBe('hello ide e2e');

    await switchToChatFromIde(page);
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();
  });

  test('chat roundtrip preserves editor undo stack', async ({ page }) => {
    stubIdeWorkspaceRoutes(page);

    await page.goto('/');
    await expectChatModeWithHiddenIdeLayer(page);

    const cmContent = await switchToIdeAndOpenStubFile(page);
    await cmContent.click();
    await page.keyboard.press('Control+a');
    await page.keyboard.type('alpha');

    await switchToChatFromIde(page);

    const ide = await switchToIdeFromChat(page);
    const cmAfter = ide.getByTestId('ide-editor-cm').locator('.cm-content');
    await expect(cmAfter).toBeVisible({ timeout: 10_000 });
    await expect(cmAfter).toContainText('alpha');
    await ensureNoIdeConfirmBlocking(page);

    await cmAfter.focus();
    await page.keyboard.press('Control+z');
    await expect(cmAfter).toContainText('hello ide');
    await expect(cmAfter).not.toContainText('alpha');
  });
});

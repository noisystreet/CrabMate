import { expect, test } from '@playwright/test';
import { installChatStreamStub } from './fix-chat-stream';

const GIT_TOOL_ENVELOPE = JSON.stringify({
  crabmate_tool: {
    v: 1,
    name: 'git_status',
    summary: 'git status',
    ok: true,
    output: 'git status (exit=0):\n位于分支 main',
    tool_call_id: 'e2e-call',
  },
});

/**
 * Stub conversation snapshot so refresh/hydrate shows a formatted tool card (not raw JSON).
 */
async function installConversationMessagesStub(page: import('@playwright/test').Page): Promise<void> {
  await page.route('**/conversation/messages?**', async (route) => {
    if (route.request().method() !== 'GET') {
      await route.continue();
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        conversation_id: 'e2e-conv',
        revision: 1,
        messages: [
          { role: 'user', content: 'e2e hydrate' },
          {
            role: 'tool',
            name: 'git_status',
            content: GIT_TOOL_ENVELOPE,
            display_content: 'git_status · git status',
            display_reasoning_content: 'tool: git_status\ngit status (exit=0):\n位于分支 main',
          },
        ],
      }),
    });
  });
}

test.describe('Web UI smoke', () => {
  test('GET /health returns JSON with status and checks', async ({ request }) => {
    const res = await request.get('/health');
    expect(res.ok()).toBeTruthy();
    const body = (await res.json()) as { status: string; checks: Record<string, { ok: boolean }> };
    expect(typeof body.status).toBe('string');
    expect(['ok', 'degraded']).toContain(body.status);
    expect(body.checks).toBeDefined();
    expect(typeof body.checks).toBe('object');
  });

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

  test('hydrated tool card after reload is not raw JSON', async ({ page }) => {
    await installChatStreamStub(page);
    await installConversationMessagesStub(page);
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    const input = page.getByTestId('chat-composer-input');
    await input.fill('e2e hydrate reload');
    await page.getByTestId('chat-send-button').click();
    await expect(page.getByTestId('chat-tool-card').first()).toBeVisible({ timeout: 30_000 });

    await page.reload();
    const toolCard = page.getByTestId('chat-tool-card').first();
    await expect(toolCard).toBeVisible({ timeout: 30_000 });
    await expect(toolCard).toContainText('git status');
    await expect(toolCard).not.toContainText('crabmate_tool');
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

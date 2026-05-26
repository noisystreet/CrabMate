import { expect, test } from '@playwright/test';

import {
  expectAssistantText,
  expectToolCardVisible,
  installChatStreamStub,
  installConversationMessagesStub,
  putActiveSessionWithServerConversation,
  putFreshLocalSession,
  sendStubMessage,
} from './helpers';

test.describe('Web UI smoke', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_smoke');
  });

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

    await sendStubMessage(page, 'e2e ping');
    await expectAssistantText(page, 'Hello from E2E stub');
    await expectToolCardVisible(page);
  });

  test('hydrated tool card after reload is not raw JSON', async ({ page, request }) => {
    await installChatStreamStub(page);
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await sendStubMessage(page, 'e2e hydrate reload');
    await expectToolCardVisible(page);

    await putActiveSessionWithServerConversation(request, 's_e2e_smoke', 'e2e-conv', {
      title: 'E2E smoke',
    });
    await installConversationMessagesStub(page);
    await page.reload();
    const toolCard = page.getByTestId('chat-tool-card').first();
    await expect(toolCard).toBeVisible();
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

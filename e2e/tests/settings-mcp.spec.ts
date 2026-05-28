import { expect, test } from '@playwright/test';

import { UI_TIMEOUT } from './helpers/assertions';
import { openSettingsPage, openSettingsSection, putFreshLocalSession } from './helpers';

test.describe('settings page MCP', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_settings_mcp');
  });

  test('MCP section save persists slug from name via user-data API', async ({
    page,
    request,
  }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await openSettingsPage(page);
    await openSettingsSection(page, 'mcp');
    await expect(page.getByTestId('settings-mcp-block')).toBeVisible();

    const mcpJson = JSON.stringify({
      mcpServers: {
        'playwright-mcp': { command: 'true', args: [] },
      },
    });
    await page.getByTestId('settings-mcp-import-json').fill(mcpJson);
    await page.getByTestId('settings-mcp-import-apply').click();
    await expect(page.getByText(/已导入 1 个|Imported 1 MCP/i)).toBeVisible();
    const row = page.locator('.settings-mcp-server-row').last();
    await row.locator('.settings-field').first().locator('input').fill('Playwright MCP');
    const serverEnabled = row.getByTestId('settings-mcp-server-enabled');
    if ((await serverEnabled.getAttribute('aria-checked')) === 'true') {
      await serverEnabled.click();
    }

    const saveResp = page.waitForResponse(
      (res) =>
        res.url().includes('/user-data/mcp-servers') &&
        res.request().method() === 'PUT' &&
        res.status() === 204,
    );
    await page.getByTestId('settings-mcp-save').click();
    await saveResp;

    const get = await request.get('/user-data/mcp-servers');
    expect(get.ok()).toBeTruthy();
    const body = (await get.json()) as {
      servers: { name: string; slug: string; has_command: boolean }[];
    };
    const saved = body.servers.find((s) => s.name === 'Playwright MCP');
    expect(saved).toBeDefined();
    expect(saved!.slug).toBe('playwright_mcp');
    expect(saved!.has_command).toBe(true);
  });

  test('import MCP JSON adds server rows', async ({ page, request }) => {
    const mcpJson = JSON.stringify({
      mcpServers: {
        'e2e-import': {
          command: 'npx',
          args: ['-y', 'echo', 'mcp-e2e'],
        },
      },
    });

    await page.goto('/');
    await openSettingsPage(page);
    await openSettingsSection(page, 'mcp');
    await page.getByTestId('settings-mcp-import-json').fill(mcpJson);
    await page.getByTestId('settings-mcp-import-apply').click();
    await expect(page.getByText(/已导入 1 个|Imported 1 MCP/i)).toBeVisible();

    const importedRow = page.locator('.settings-mcp-server-row').last();
    await expect(importedRow.locator('.settings-field input.settings-input').first()).toHaveValue(
      'E2e Import',
    );
    await expect(importedRow.locator('input.settings-input-mono')).toHaveCount(0);

    const saveResp = page.waitForResponse(
      (res) =>
        res.url().includes('/user-data/mcp-servers') &&
        res.request().method() === 'PUT' &&
        res.status() === 204,
    );
    await page.getByTestId('settings-mcp-save').click();
    await saveResp;
    const get = await request.get('/user-data/mcp-servers');
    const body = (await get.json()) as {
      servers: { name: string; has_command: boolean }[];
    };
    const imported = body.servers.find((s) => s.name === 'E2e Import');
    expect(imported?.has_command).toBe(true);
  });

  test('MCP nav sets URL hash', async ({ page }) => {
    await page.goto('/');
    await openSettingsPage(page);
    await openSettingsSection(page, 'mcp');
    await expect(page).toHaveURL(/#settings\/mcp/, { timeout: UI_TIMEOUT });
    await expect(page.getByTestId('settings-mcp-block')).toBeVisible();
    await expect(page.getByTestId('settings-nav-mcp')).toHaveClass(/active/);
  });
});

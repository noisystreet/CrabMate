import { expect, test } from '@playwright/test';

import {
  closeSettingsPage,
  openSettingsPage,
  openSettingsSection,
  putFreshLocalSession,
  saveSettingsPage,
} from './helpers';

const E2E_API_KEY_PLACEHOLDER = 'E2E_STUB_CLIENT_KEY_NOT_REAL';

test.describe('settings page LLM', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_settings_llm');
  });

  test('model and client API key save (secret PUT stubbed)', async ({ page }) => {
    let secretPut: { api_key?: string } | null = null;
    await page.route('**/user-data/secrets/client-llm', async (route) => {
      if (route.request().method() === 'PUT') {
        secretPut = route.request().postDataJSON() as { api_key?: string };
        await route.fulfill({ status: 204, body: '' });
        return;
      }
      await route.continue();
    });

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await openSettingsPage(page);
    await openSettingsSection(page, 'llm');
    await page.getByTestId('settings-llm-model').fill('e2e-test-model');
    await page.getByTestId('settings-client-api-key').fill(E2E_API_KEY_PLACEHOLDER);

    await saveSettingsPage(page);
    await closeSettingsPage(page);

    expect(secretPut?.api_key).toBe(E2E_API_KEY_PLACEHOLDER);

    await openSettingsPage(page);
    await openSettingsSection(page, 'llm');
    await expect(page.getByTestId('settings-llm-model')).toHaveValue('e2e-test-model');
    await expect(page.getByTestId('settings-client-api-key')).toHaveValue('');
  });
});

import { expect, type Locator, type Page } from '@playwright/test';

import { UI_TIMEOUT } from './assertions';

/** 设置全屏层仅在带 `settings-page-visible` 时可交互（见 `modal.css`）。 */
export function settingsPageVisible(page: Page): Locator {
  return page.locator('[data-testid="settings-page"].settings-page-visible');
}

export async function openSettingsPage(page: Page): Promise<void> {
  await page.getByTestId('settings-open').click();
  await expect(settingsPageVisible(page)).toBeVisible({ timeout: UI_TIMEOUT });
}

export async function openSettingsSection(
  page: Page,
  section: 'appearance' | 'llm' | 'mcp' | 'tools' | 'session' | 'shortcuts' | 'executor-llm',
): Promise<void> {
  await page.getByTestId(`settings-nav-${section}`).click();
}

export async function saveSettingsPage(page: Page): Promise<void> {
  const save = page.getByTestId('settings-save-all');
  await expect(save).toBeEnabled({ timeout: UI_TIMEOUT });
  await save.click();
  await expect(page.locator('.settings-save-feedback-global')).toContainText(
    /已保存全部设置|All settings saved/i,
    { timeout: UI_TIMEOUT },
  );
}

export async function closeSettingsPage(page: Page): Promise<void> {
  await page.getByTestId('settings-back').click();
  await expect(settingsPageVisible(page)).toHaveCount(0, { timeout: UI_TIMEOUT });
}

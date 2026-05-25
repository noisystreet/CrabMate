import { expect, type Page } from '@playwright/test';

/** 在会话列表空白处右键打开 rail 菜单并进入「管理会话」模态。 */
export async function openSessionListModal(page: Page): Promise<void> {
  const rail = page.locator('.nav-rail-scroll');
  await rail.click({ button: 'right' });
  await page.getByRole('menuitem', { name: /管理会话|Manage sessions/i }).click();
  await expect(page.getByTestId('session-list-modal')).toBeVisible();
}

import { expect, type Page } from '@playwright/test';

/** 展开「规划 / 工具时间线」面板（默认可能折叠）。 */
export async function expandTimelinePanel(page: Page): Promise<void> {
  const toggle = page.getByTestId('timeline-panel-toggle');
  const expanded = await toggle.getAttribute('aria-expanded');
  if (expanded !== 'true') {
    await toggle.click();
  }
  await expect(page.getByTestId('timeline-panel')).toBeVisible();
}

import { expect, test } from '@playwright/test';

import {
  expandTimelinePanel,
  installChatStreamStub,
  putFreshLocalSession,
  sendStubMessage,
} from './helpers';

test.describe('SSE staged plan timeline', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_plan');
  });

  test('staged_plan_step events appear in timeline panel', async ({ page }) => {
    await installChatStreamStub(page, { preset: 'staged_plan' });

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await sendStubMessage(page, 'e2e staged plan');
    await expandTimelinePanel(page);

    const panel = page.getByTestId('timeline-panel');
    await expect(panel).toContainText(/1\.\s*(开始|start)/);
    await expect(panel).toContainText(/1\.\s*(完成|done)/);
  });
});

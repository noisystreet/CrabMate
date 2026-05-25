import { expect, test } from '@playwright/test';

import {
  PAGINATE_CONV_ID,
  PAGINATE_TOTAL,
  putActiveSessionWithServerConversation,
  seedPaginatedConversation,
} from './helpers';

test.describe('conversation load older (UI click)', () => {
  test.beforeAll(async ({ request }) => {
    await seedPaginatedConversation(request);
  });

  test('click load older fetches older page and hides control', async ({ page, request }) => {
    await putActiveSessionWithServerConversation(request, 's_e2e_load_older', PAGINATE_CONV_ID, {
      title: 'E2E load older click',
    });

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();
    await expect(page.getByText(`e2e-msg-${PAGINATE_TOTAL - 1}`, { exact: true })).toBeVisible({
      timeout: 30_000,
    });

    const loadOlder = page.getByTestId('chat-load-older');
    await expect(loadOlder).toBeVisible();

    const olderPage = page.waitForResponse(
      (res) =>
        res.ok() &&
        res.request().method() === 'GET' &&
        res.url().includes('/conversation/messages') &&
        res.url().includes('before_index'),
    );
    await loadOlder.click({ force: true });
    const res = await olderPage;
    const loaded = (await res.json()) as {
      window_start_index: number;
      has_older: boolean;
      messages: { content: string }[];
    };
    expect(loaded.window_start_index).toBe(0);
    expect(loaded.has_older).toBe(false);
    expect(loaded.messages[0]?.content).toBe('e2e-msg-0');

    await expect(loadOlder).not.toBeVisible();
  });
});

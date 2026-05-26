import { expect, test } from '@playwright/test';

import {
  installChatStreamStub,
  openSessionListModal,
  putFreshLocalSession,
  putWorkspaceSessions,
  typeComposerDraft,
  UI_TIMEOUT,
} from './helpers';

test.describe('keyboard shortcuts', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_keys');
  });

  test('Enter in composer sends message (stub stream)', async ({ page }) => {
    await installChatStreamStub(page);
    await page.goto('/');
    await expect(page.getByTestId('chat-composer-input')).toBeVisible();

    const streamDone = page.waitForResponse(
      (res) => res.url().includes('/chat/stream') && res.request().method() === 'POST',
      { timeout: UI_TIMEOUT },
    );
    await typeComposerDraft(page, 'e2e enter send');
    await page.getByTestId('chat-composer-input').press('Enter');
    await streamDone;

    await expect(
      page.getByTestId('chat-message-row').filter({ hasText: 'Hello from E2E stub' }),
    ).toBeVisible();
  });

  test('Escape closes session list modal', async ({ page }) => {
    await page.goto('/');
    await openSessionListModal(page);
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('session-list-modal')).not.toBeVisible();
  });

  test('End key scrolls messages toward bottom', async ({ page, request }) => {
    await putWorkspaceSessions(
      request,
      [
        {
          id: 's_e2e_keys',
          title: 'E2E scroll',
          draft: '',
          messages: Array.from({ length: 40 }, (_, i) => ({
            id: `m_scroll_${i}`,
            role: 'user',
            text: `e2e-scroll-line-${i}`,
          })),
          updated_at: 1,
          pinned: false,
          starred: false,
        },
      ],
      's_e2e_keys',
    );

    await page.goto('/');
    await expect(page.getByText('e2e-scroll-line-0')).toBeVisible();

    const scroller = page.getByTestId('chat-messages-scroller');
    await scroller.evaluate((el) => {
      el.scrollTop = 0;
    });
    expect(await scroller.evaluate((el) => el.scrollTop)).toBe(0);

    await page.getByTestId('chat-composer-input').focus();
    await page.getByTestId('chat-composer-input').press('End');

    await expect
      .poll(async () => {
        const st = await scroller.evaluate((el) => ({
          top: el.scrollTop,
          max: el.scrollHeight - el.clientHeight,
        }));
        return st.max > 0 && st.top >= st.max - 4;
      }, { timeout: UI_TIMEOUT })
      .toBe(true);
  });
});

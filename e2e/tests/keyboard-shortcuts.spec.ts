import { expect, test } from '@playwright/test';

import {
  installChatStreamStub,
  openSessionListModal,
  putFreshLocalSession,
  putWorkspaceSessions,
  SCROLL_TIMEOUT,
  sendAndWait,
  typeComposerDraft,
  UI_TIMEOUT,
  visibleChatLayer,
} from './helpers';

test.describe('keyboard shortcuts', () => {
  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, 's_e2e_keys');
  });

  test('Enter in composer sends message (stub stream)', async ({ page }) => {
    await installChatStreamStub(page);
    await page.goto('/');
    await expect(page.getByTestId('chat-composer-input')).toBeVisible();

    await typeComposerDraft(page, 'e2e enter send');
    await sendAndWait(page, () => page.getByTestId('chat-composer-input').press('Enter'));

    await expect(
      visibleChatLayer(page).getByTestId('chat-message-row').filter({ hasText: 'Hello from E2E stub' }),
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
    await expect(visibleChatLayer(page).getByText('e2e-scroll-line-0')).toBeVisible();

    const scroller = visibleChatLayer(page).getByTestId('chat-messages-scroller');
    await page.getByTestId('chat-composer-input').focus();
    await page.getByTestId('chat-composer-input').press('Home');
    await expect
      .poll(async () => scroller.evaluate((el) => el.scrollTop), { timeout: SCROLL_TIMEOUT })
      .toBe(0);

    await page.getByTestId('chat-composer-input').press('End');

    await expect
      .poll(async () => {
        const st = await scroller.evaluate((el) => ({
          top: el.scrollTop,
          max: el.scrollHeight - el.clientHeight,
        }));
        return st.max > 0 && st.top >= st.max - 4;
      }, { timeout: SCROLL_TIMEOUT })
      .toBe(true);
  });

  test('Enter send scrolls messages toward bottom after scrolled up', async ({
    page,
    request,
  }) => {
    await putWorkspaceSessions(
      request,
      [
        {
          id: 's_e2e_keys',
          title: 'E2E send scroll',
          draft: '',
          messages: Array.from({ length: 40 }, (_, i) => ({
            id: `m_send_scroll_${i}`,
            role: 'user',
            text: `e2e-send-scroll-line-${i}`,
          })),
          updated_at: 1,
          pinned: false,
          starred: false,
        },
      ],
      's_e2e_keys',
    );

    await installChatStreamStub(page);
    await page.goto('/');
    await expect(visibleChatLayer(page).getByText('e2e-send-scroll-line-0')).toBeVisible();

    const scroller = visibleChatLayer(page).getByTestId('chat-messages-scroller');
    await page.getByTestId('chat-composer-input').focus();
    await page.getByTestId('chat-composer-input').press('Home');
    await expect
      .poll(async () => scroller.evaluate((el) => el.scrollTop), { timeout: SCROLL_TIMEOUT })
      .toBe(0);

    await typeComposerDraft(page, 'e2e send scroll follow');
    await sendAndWait(page, () => page.getByTestId('chat-composer-input').press('Enter'));

    await expect(
      visibleChatLayer(page).getByTestId('chat-message-row').filter({ hasText: 'Hello from E2E stub' }),
    ).toBeVisible();

    await expect
      .poll(async () => {
        const st = await scroller.evaluate((el) => ({
          top: el.scrollTop,
          max: el.scrollHeight - el.clientHeight,
        }));
        return st.max > 0 && st.top >= st.max - 4;
      }, { timeout: SCROLL_TIMEOUT })
      .toBe(true);
  });
});

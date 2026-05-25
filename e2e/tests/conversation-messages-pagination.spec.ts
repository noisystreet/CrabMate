import { expect, test } from '@playwright/test';

import {
  getConversationMessages,
  PAGINATE_CONV_ID,
  PAGINATE_PAGE_LIMIT,
  PAGINATE_TOTAL,
  putActiveSessionWithServerConversation,
  seedPaginatedConversation,
} from './helpers';

test.describe.configure({ mode: 'serial' });

test.describe('conversation messages pagination and hydration', () => {
  test.beforeAll(async ({ request }) => {
    await seedPaginatedConversation(request);
  });

  test('full window when limit omitted', async ({ request }) => {
    const body = await getConversationMessages(request, PAGINATE_CONV_ID, '');
    expect(body.conversation_id).toBe(PAGINATE_CONV_ID);
    expect(body.total_count).toBe(PAGINATE_TOTAL);
    expect(body.window_start_index).toBe(0);
    expect(body.has_older).toBe(false);
    expect(body.messages).toHaveLength(PAGINATE_TOTAL);
    expect(body.messages[0].content).toBe('e2e-msg-0');
    expect(body.messages[PAGINATE_TOTAL - 1].content).toBe(`e2e-msg-${PAGINATE_TOTAL - 1}`);
  });

  test('tail page limit=80', async ({ request }) => {
    const body = await getConversationMessages(request, PAGINATE_CONV_ID, `limit=${PAGINATE_PAGE_LIMIT}`);
    expect(body.total_count).toBe(PAGINATE_TOTAL);
    expect(body.window_start_index).toBe(PAGINATE_TOTAL - PAGINATE_PAGE_LIMIT);
    expect(body.has_older).toBe(true);
    expect(body.messages).toHaveLength(PAGINATE_PAGE_LIMIT);
    expect(body.messages[0].content).toBe(`e2e-msg-${PAGINATE_TOTAL - PAGINATE_PAGE_LIMIT}`);
    expect(body.messages[PAGINATE_PAGE_LIMIT - 1].content).toBe(`e2e-msg-${PAGINATE_TOTAL - 1}`);
  });

  test('older page before_index=window_start', async ({ request }) => {
    const tail = await getConversationMessages(request, PAGINATE_CONV_ID, `limit=${PAGINATE_PAGE_LIMIT}`);
    const body = await getConversationMessages(
      request,
      PAGINATE_CONV_ID,
      `limit=${PAGINATE_PAGE_LIMIT}&before_index=${tail.window_start_index}`,
    );
    expect(body.total_count).toBe(PAGINATE_TOTAL);
    expect(body.window_start_index).toBe(0);
    expect(body.has_older).toBe(false);
    expect(body.messages).toHaveLength(PAGINATE_TOTAL - PAGINATE_PAGE_LIMIT);
    expect(body.messages[0].content).toBe('e2e-msg-0');
    expect(body.messages[body.messages.length - 1].content).toBe(
      `e2e-msg-${PAGINATE_TOTAL - PAGINATE_PAGE_LIMIT - 1}`,
    );
  });

  test('unknown conversation returns 404', async ({ request }) => {
    const res = await request.get('/conversation/messages?conversation_id=e2e-no-such-conv');
    expect(res.status()).toBe(404);
    const err = (await res.json()) as { code: string };
    expect(err.code).toBe('CONVERSATION_NOT_FOUND');
  });

  test('hydrate tail page shows latest and load-older control', async ({ page, request }) => {
    await putActiveSessionWithServerConversation(request, 's_e2e_hydrate', PAGINATE_CONV_ID, {
      title: 'E2E hydrate paginate',
    });

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    // 须 exact：否则「e2e-msg-0」会子串命中 e2e-msg-10 / e2e-msg-20 …
    await expect(
      page.getByRole('button', { name: /加载更早的消息|Load older messages/i }),
    ).toBeVisible({ timeout: 30_000 });
    await expect(page.getByText(`e2e-msg-${PAGINATE_TOTAL - 1}`, { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await expect(page.getByText('e2e-msg-0', { exact: true })).not.toBeVisible();
  });
});

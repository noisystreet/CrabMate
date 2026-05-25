import { expect, test } from '@playwright/test';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

const CONV_ID = 'e2e-paginate-conv';
const TOTAL = 100;
const PAGE_LIMIT = 80;

type MessagesPage = {
  conversation_id: string;
  revision: number;
  messages: { role: string; content: string }[];
  total_count: number;
  window_start_index: number;
  has_older: boolean;
};

function userMessages(n: number): { role: string; content: string }[] {
  return Array.from({ length: n }, (_, i) => ({
    role: 'user',
    content: `e2e-msg-${i}`,
  }));
}

async function seedPaginatedConversation(request: import('@playwright/test').APIRequestContext): Promise<void> {
  const ws = fs.mkdtempSync(path.join(os.tmpdir(), 'crabmate-e2e-ws-'));
  const setWs = await request.post('/workspace', { data: { path: ws } });
  expect(setWs.ok()).toBeTruthy();

  const seed = await request.post('/e2e/fixtures/conversation', {
    data: {
      conversation_id: CONV_ID,
      messages: userMessages(TOTAL),
      replace: true,
    },
  });
  expect(seed.status(), await seed.text()).toBe(204);
}

async function getMessages(
  request: import('@playwright/test').APIRequestContext,
  query: string,
): Promise<MessagesPage> {
  const res = await request.get(`/conversation/messages?conversation_id=${CONV_ID}&${query}`);
  expect(res.ok(), await res.text()).toBeTruthy();
  return (await res.json()) as MessagesPage;
}

test.describe.configure({ mode: 'serial' });

test.describe('conversation messages pagination and hydration', () => {
  test.beforeAll(async ({ request }) => {
    await seedPaginatedConversation(request);
  });

  test('full window when limit omitted', async ({ request }) => {
    const body = await getMessages(request, '');
    expect(body.conversation_id).toBe(CONV_ID);
    expect(body.total_count).toBe(TOTAL);
    expect(body.window_start_index).toBe(0);
    expect(body.has_older).toBe(false);
    expect(body.messages).toHaveLength(TOTAL);
    expect(body.messages[0].content).toBe('e2e-msg-0');
    expect(body.messages[TOTAL - 1].content).toBe(`e2e-msg-${TOTAL - 1}`);
  });

  test('tail page limit=80', async ({ request }) => {
    const body = await getMessages(request, `limit=${PAGE_LIMIT}`);
    expect(body.total_count).toBe(TOTAL);
    expect(body.window_start_index).toBe(TOTAL - PAGE_LIMIT);
    expect(body.has_older).toBe(true);
    expect(body.messages).toHaveLength(PAGE_LIMIT);
    expect(body.messages[0].content).toBe(`e2e-msg-${TOTAL - PAGE_LIMIT}`);
    expect(body.messages[PAGE_LIMIT - 1].content).toBe(`e2e-msg-${TOTAL - 1}`);
  });

  test('older page before_index=window_start', async ({ request }) => {
    const tail = await getMessages(request, `limit=${PAGE_LIMIT}`);
    const body = await getMessages(request, `limit=${PAGE_LIMIT}&before_index=${tail.window_start_index}`);
    expect(body.total_count).toBe(TOTAL);
    expect(body.window_start_index).toBe(0);
    expect(body.has_older).toBe(false);
    expect(body.messages).toHaveLength(TOTAL - PAGE_LIMIT);
    expect(body.messages[0].content).toBe('e2e-msg-0');
    expect(body.messages[body.messages.length - 1].content).toBe(`e2e-msg-${TOTAL - PAGE_LIMIT - 1}`);
  });

  test('unknown conversation returns 404', async ({ request }) => {
    const res = await request.get('/conversation/messages?conversation_id=e2e-no-such-conv');
    expect(res.status()).toBe(404);
    const err = (await res.json()) as { code: string };
    expect(err.code).toBe('CONVERSATION_NOT_FOUND');
  });

  test('hydrate tail page shows latest and load-older control', async ({ page, request }) => {
    await request.put('/user-data/workspaces/current/sessions', {
      data: {
        sessions: [
          {
            id: 's_e2e_hydrate',
            title: 'E2E hydrate paginate',
            draft: '',
            messages: [],
            updated_at: 1,
            pinned: false,
            starred: false,
            server_conversation_id: CONV_ID,
            server_revision: 1,
          },
        ],
        active_session_id: 's_e2e_hydrate',
      },
    });

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    // 须 exact：否则「e2e-msg-0」会子串命中 e2e-msg-10 / e2e-msg-20 …
    await expect(page.getByText(`e2e-msg-${TOTAL - 1}`, { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await expect(page.getByText('e2e-msg-0', { exact: true })).not.toBeVisible();
    await expect(
      page.getByRole('button', { name: /加载更早的消息|Load older messages/i }),
    ).toBeVisible();
  });
});

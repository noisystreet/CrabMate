import { expect, type APIRequestContext } from '@playwright/test';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

import { userMessages } from './messages';

export const PAGINATE_CONV_ID = 'e2e-paginate-conv';
export const PAGINATE_TOTAL = 100;
export const PAGINATE_PAGE_LIMIT = 80;

export type SeedConversationOptions = {
  conversationId: string;
  messages: { role: string; content: string }[];
  replace?: boolean;
  /** 为 true 时 POST /workspace 指向新的临时目录（默认 true）。 */
  isolateWorkspace?: boolean;
};

/**
 * 通过 E2E 夹具路由写入持久化会话（须服务端 `CM_E2E_FIXTURES=1`）。
 */
export async function seedConversation(
  request: APIRequestContext,
  options: SeedConversationOptions,
): Promise<void> {
  if (options.isolateWorkspace !== false) {
    const ws = fs.mkdtempSync(path.join(os.tmpdir(), 'crabmate-e2e-ws-'));
    const setWs = await request.post('/workspace', { data: { path: ws } });
    expect(setWs.ok()).toBeTruthy();
  }

  const seed = await request.post('/e2e/fixtures/conversation', {
    data: {
      conversation_id: options.conversationId,
      messages: options.messages,
      replace: options.replace ?? true,
    },
  });
  expect(seed.status(), await seed.text()).toBe(204);
}

/** 默认 100 条 user 消息，供分页 API / 水合用例复用。 */
export async function seedPaginatedConversation(request: APIRequestContext): Promise<void> {
  await seedConversation(request, {
    conversationId: PAGINATE_CONV_ID,
    messages: userMessages(PAGINATE_TOTAL),
    replace: true,
  });
}

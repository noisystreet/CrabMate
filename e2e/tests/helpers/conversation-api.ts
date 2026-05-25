import { expect, type APIRequestContext } from '@playwright/test';

import type { MessagesPage } from './messages';

export async function getConversationMessages(
  request: APIRequestContext,
  conversationId: string,
  query = '',
): Promise<MessagesPage> {
  const q = query ? `&${query}` : '';
  const res = await request.get(`/conversation/messages?conversation_id=${conversationId}${q}`);
  expect(res.ok(), await res.text()).toBeTruthy();
  return (await res.json()) as MessagesPage;
}

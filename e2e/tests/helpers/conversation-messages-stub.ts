import type { Page } from '@playwright/test';

const GIT_TOOL_ENVELOPE = JSON.stringify({
  crabmate_tool: {
    v: 1,
    name: 'git_status',
    summary: 'git status',
    ok: true,
    output: 'git status (exit=0):\n位于分支 main',
    tool_call_id: 'e2e-call',
  },
});

/**
 * Stub GET /conversation/messages so refresh/hydrate shows a formatted tool card (not raw JSON).
 */
export async function installConversationMessagesStub(
  page: Page,
  conversationId = 'e2e-conv',
): Promise<void> {
  await page.route('**/conversation/messages?**', async (route) => {
    if (route.request().method() !== 'GET') {
      await route.continue();
      return;
    }
    const url = new URL(route.request().url());
    if (url.searchParams.get('conversation_id') !== conversationId) {
      await route.continue();
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        conversation_id: conversationId,
        revision: 1,
        total_count: 2,
        window_start_index: 0,
        has_older: false,
        messages: [
          { role: 'user', content: 'e2e hydrate' },
          {
            role: 'tool',
            name: 'git_status',
            content: GIT_TOOL_ENVELOPE,
            display_content: 'git_status · git status',
            display_reasoning_content: 'tool: git_status\ngit status (exit=0):\n位于分支 main',
          },
        ],
      }),
    });
  });
}

/**
 * Stubs POST /chat/stream with a minimal SSE sequence so the UI can render
 * assistant text and a tool card without calling a real LLM.
 */
import type { Page, Route } from '@playwright/test';

const SSE_V = 1;

export type ChatStreamStubOptions = {
  conversationId?: string;
  streamJobId?: string;
  /** 纯文本 delta（默认 Hello from E2E stub.） */
  assistantDelta?: string;
  /** 完整 SSE 正文；若提供则忽略 assistantDelta 与默认 tool_result。 */
  body?: string;
};

function defaultSseBody(opts: ChatStreamStubOptions): string {
  const delta = opts.assistantDelta ?? 'Hello from E2E stub.';
  const events = [
    `id: 1\ndata: {"sse_capabilities":{"supported_sse_v":${SSE_V}}}\n\n`,
    `id: 2\ndata: {"v":${SSE_V}}\n\n`,
    `id: 3\ndata: ${delta}\n\n`,
    `id: 4\ndata: {"tool_result":{"name":"diagnostic_summary","result_version":1,"summary":"ok","output":"stub","ok":true}}\n\n`,
    `id: 5\ndata: {"stream_ended":{"reason":"completed"}}\n\n`,
  ];
  return events.join('');
}

export async function installChatStreamStub(page: Page, opts: ChatStreamStubOptions = {}): Promise<void> {
  const conversationId = opts.conversationId ?? 'e2e-conv';
  const streamJobId = opts.streamJobId ?? '1';
  const body = opts.body ?? defaultSseBody(opts);

  await page.route('**/chat/stream', async (route: Route) => {
    if (route.request().method() !== 'POST') {
      await route.continue();
      return;
    }

    await route.fulfill({
      status: 200,
      headers: {
        'content-type': 'text/event-stream; charset=utf-8',
        'cache-control': 'no-cache',
        'x-conversation-id': conversationId,
        'x-stream-job-id': streamJobId,
      },
      body,
    });
  });
}

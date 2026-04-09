/**
 * Stubs POST /chat/stream with a minimal SSE sequence so the UI can render
 * assistant text and a tool card without calling a real LLM.
 */
import type { Page, Route } from '@playwright/test';

const SSE_V = 1;

export async function installChatStreamStub(page: Page): Promise<void> {
  await page.route('**/chat/stream', async (route: Route) => {
    if (route.request().method() !== 'POST') {
      await route.continue();
      return;
    }

    const headers = {
      'content-type': 'text/event-stream; charset=utf-8',
      'cache-control': 'no-cache',
      'x-conversation-id': 'e2e-conv',
      'x-stream-job-id': '1',
    };

    const events = [
      `id: 1\ndata: {"sse_capabilities":{"supported_sse_v":${SSE_V}}}\n\n`,
      `id: 2\ndata: {"v":${SSE_V}}\n\n`,
      `id: 3\ndata: Hello from E2E stub.\n\n`,
      `id: 4\ndata: {"tool_result":{"name":"diagnostic_summary","result_version":1,"summary":"ok","output":"stub","ok":true}}\n\n`,
      `id: 5\ndata: {"stream_ended":{"reason":"completed"}}\n\n`,
    ];

    await route.fulfill({
      status: 200,
      headers,
      body: events.join(''),
    });
  });
}

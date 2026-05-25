/**
 * Stubs POST /chat/stream with SSE sequences so the UI can render without a real LLM.
 */
import type { Page, Route } from '@playwright/test';

const SSE_V = 1;

export type ChatStreamStubOptions = {
  conversationId?: string;
  streamJobId?: string;
  /** 纯文本 delta（默认 Hello from E2E stub.） */
  assistantDelta?: string;
  /** 完整 SSE 正文；若提供则忽略其它预设。 */
  body?: string;
  /** 预设：默认助手+工具结果 / 流错误 stop / 命令审批 */
  preset?: 'default' | 'stream_error' | 'command_approval';
};

function sseEvent(id: number, data: string): string {
  return `id: ${id}\ndata: ${data}\n\n`;
}

function sseJson(payload: Record<string, unknown>): string {
  return JSON.stringify(payload);
}

/** 默认：助手正文 + diagnostic_summary 工具卡 + stream_ended */
export function buildDefaultStreamBody(opts: ChatStreamStubOptions = {}): string {
  const delta = opts.assistantDelta ?? 'Hello from E2E stub.';
  return [
    sseEvent(1, sseJson({ sse_capabilities: { supported_sse_v: SSE_V } })),
    sseEvent(2, sseJson({ v: SSE_V })),
    sseEvent(3, delta),
    sseEvent(
      4,
      sseJson({
        tool_result: {
          name: 'diagnostic_summary',
          result_version: 1,
          summary: 'ok',
          output: 'stub',
          ok: true,
        },
      }),
    ),
    sseEvent(5, sseJson({ stream_ended: { reason: 'completed' } })),
  ].join('');
}

/** 控制面 error+code → stop，状态栏应显示失败。 */
export function buildStreamErrorBody(): string {
  return [
    sseEvent(1, sseJson({ sse_capabilities: { supported_sse_v: SSE_V } })),
    sseEvent(2, sseJson({ v: SSE_V })),
    sseEvent(3, sseJson({ error: 'e2e intentional failure', code: 'E2E_STREAM_FAIL' })),
    sseEvent(4, sseJson({ stream_ended: { reason: 'error' } })),
  ].join('');
}

/** command_approval_request → 审批弹窗。 */
export function buildCommandApprovalStreamBody(): string {
  return [
    sseEvent(1, sseJson({ sse_capabilities: { supported_sse_v: SSE_V } })),
    sseEvent(2, sseJson({ v: SSE_V })),
    sseEvent(3, 'e2e approval ping.'),
    sseEvent(
      4,
      sseJson({
        command_approval_request: {
          command: 'git',
          args: 'status',
          allowlist_key: 'git',
        },
      }),
    ),
    sseEvent(5, sseJson({ stream_ended: { reason: 'completed' } })),
  ].join('');
}

function bodyForPreset(opts: ChatStreamStubOptions): string {
  switch (opts.preset) {
    case 'stream_error':
      return buildStreamErrorBody();
    case 'command_approval':
      return buildCommandApprovalStreamBody();
    default:
      return buildDefaultStreamBody(opts);
  }
}

export async function installChatStreamStub(page: Page, opts: ChatStreamStubOptions = {}): Promise<void> {
  const conversationId = opts.conversationId ?? 'e2e-conv';
  const streamJobId = opts.streamJobId ?? '1';
  const body = opts.body ?? bodyForPreset(opts);

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

/** 审批按钮提交（避免 E2E 打到真实后端逻辑）。 */
export async function installChatApprovalStub(page: Page): Promise<void> {
  await page.route('**/chat/approval', async (route) => {
    if (route.request().method() !== 'POST') {
      await route.continue();
      return;
    }
    await route.fulfill({ status: 204, body: '' });
  });
}

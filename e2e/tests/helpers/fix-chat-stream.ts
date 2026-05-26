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
  /** 完整 SSE 正文；若提供则忽略 preset / bodies。 */
  body?: string;
  /** 预设：默认 / 流错误 / 审批 / 澄清问卷 */
  preset?: 'default' | 'stream_error' | 'command_approval' | 'clarification' | 'staged_plan';
  /** 多次 POST /chat/stream 时按顺序返回（用尽后重复最后一项）。 */
  bodies?: string[];
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

/** staged_plan_* → 时间线面板条目（与 fixtures/sse_control_golden.jsonl 对齐）。 */
export function buildStagedPlanStreamBody(): string {
  return [
    sseEvent(1, sseJson({ sse_capabilities: { supported_sse_v: SSE_V } })),
    sseEvent(2, sseJson({ v: SSE_V })),
    sseEvent(3, sseJson({ staged_plan_started: { plan_id: 'e2e-p1', total_steps: 1 } })),
    sseEvent(
      4,
      sseJson({
        staged_plan_step_started: {
          plan_id: 'e2e-p1',
          step_id: 's1',
          step_index: 1,
          total_steps: 1,
          description: 'E2E review step',
          executor_kind: 'review_readonly',
        },
      }),
    ),
    sseEvent(
      5,
      sseJson({
        staged_plan_step_finished: {
          plan_id: 'e2e-p1',
          step_id: 's1',
          step_index: 1,
          total_steps: 1,
          status: 'ok',
          executor_kind: 'review_readonly',
        },
      }),
    ),
    sseEvent(
      6,
      sseJson({
        staged_plan_finished: {
          plan_id: 'e2e-p1',
          total_steps: 1,
          completed_steps: 1,
          status: 'ok',
        },
      }),
    ),
    sseEvent(7, sseJson({ stream_ended: { reason: 'completed' } })),
  ].join('');
}

/** clarification_questionnaire → 澄清面板（与 fixtures/sse_control_golden.jsonl 对齐）。 */
export function buildClarificationStreamBody(): string {
  return [
    sseEvent(1, sseJson({ sse_capabilities: { supported_sse_v: SSE_V } })),
    sseEvent(2, sseJson({ v: SSE_V })),
    sseEvent(
      3,
      sseJson({
        clarification_questionnaire: {
          questionnaire_id: 'e2e-q1',
          intro: 'E2E please clarify',
          questions: [{ id: 'scope', label: 'Scope?', required: true }],
        },
      }),
    ),
    sseEvent(4, sseJson({ stream_ended: { reason: 'completed' } })),
  ].join('');
}

function bodyForPreset(opts: ChatStreamStubOptions): string {
  switch (opts.preset) {
    case 'stream_error':
      return buildStreamErrorBody();
    case 'command_approval':
      return buildCommandApprovalStreamBody();
    case 'clarification':
      return buildClarificationStreamBody();
    case 'staged_plan':
      return buildStagedPlanStreamBody();
    default:
      return buildDefaultStreamBody(opts);
  }
}

export async function installChatStreamStub(page: Page, opts: ChatStreamStubOptions = {}): Promise<void> {
  const conversationId = opts.conversationId ?? 'e2e-conv';
  const streamJobId = opts.streamJobId ?? '1';
  const queue = opts.bodies?.length ? [...opts.bodies] : null;
  const fallback = bodyForPreset(opts);

  await page.route('**/chat/stream', async (route: Route) => {
    if (route.request().method() !== 'POST') {
      await route.continue();
      return;
    }

    let body = fallback;
    if (queue) {
      body = queue.length > 1 ? queue.shift()! : queue[0]!;
    } else if (opts.body) {
      body = opts.body;
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

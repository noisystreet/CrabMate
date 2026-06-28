/**
 * 两轮「问候 + 技能问答」SSE 桩：含 intent_analysis 时间线与分片正文 delta，贴近用户复现路径。
 */
import type { Page, Route } from '@playwright/test';
import { createServer, type Server } from 'node:http';

const SSE_V = 1;

export const TWO_TURN_CONV_ID = 'e2e-two-turn-conv';

function sseEvent(id: number, data: string): string {
  return `id: ${id}\ndata: ${data}\n\n`;
}

function sseJson(payload: Record<string, unknown>): string {
  return JSON.stringify(payload);
}

function streamPreamble(nextId: number): { lines: string[]; nextId: number } {
  let id = nextId;
  const lines = [
    sseEvent(id++, sseJson({ sse_capabilities: { supported_sse_v: SSE_V } })),
    sseEvent(id++, sseJson({ v: SSE_V })),
    sseEvent(
      id++,
      sseJson({
        timeline_log: {
          kind: 'intent_analysis',
          title: '意图分析：问答类（直接回复）',
        },
      }),
    ),
    sseEvent(id++, sseJson({ assistant_answer_phase: true })),
  ];
  return { lines, nextId: id };
}

/** 第一轮：「你好」→ 短回复 */
export function buildGreetingTurnStreamBody(): string {
  const { lines, nextId } = streamPreamble(1);
  let id = nextId;
  lines.push(sseEvent(id++, '你'));
  lines.push(sseEvent(id++, '好'));
  lines.push(sseEvent(id++, '！我是 CrabMate 助手。'));
  lines.push(sseEvent(id++, sseJson({ stream_ended: { reason: 'completed' } })));
  return lines.join('');
}

/** 第二轮：仅正文 delta（无 intent，用于隔离水合后时间线插入问题） */
export function buildSkillsTurnStreamBodyPlainOnly(): string {
  const chunks = [
    '我可以帮你：\n',
    '1. 读写工作区文件\n',
    '2. 运行白名单命令\n',
    '（E2E stub 技能列表续写片段）',
  ];
  const { lines, nextId } = streamPreamble(1);
  let id = nextId;
  for (const c of chunks) {
    lines.push(sseEvent(id++, c));
  }
  lines.push(sseEvent(id++, sseJson({ stream_ended: { reason: 'completed' } })));
  return lines.join('');
}

/** 第二轮：「你有哪些技能」→ 较长列表（多分片 delta） */
export function buildSkillsTurnStreamBody(): string {
  const chunks = [
    '我可以帮你：\n',
    '1. 读写工作区文件\n',
    '2. 运行白名单命令\n',
    '3. 调用内置工具\n',
    '4. 分阶段规划复杂任务\n',
    '5. 搜索与会话管理\n',
    '（E2E stub 技能列表续写片段）',
  ];
  const { lines, nextId } = streamPreamble(1);
  let id = nextId;
  for (const c of chunks) {
    lines.push(sseEvent(id++, c));
  }
  lines.push(sseEvent(id++, sseJson({ stream_ended: { reason: 'completed' } })));
  return lines.join('');
}

export type TwoTurnStreamStubOptions = {
  /** 首轮 SSE 分片间隔（毫秒） */
  slowFirstTurnMs?: number;
  /** 第二轮 SSE 分片间隔（毫秒） */
  slowSecondTurnMs?: number;
  /** 第二轮跳过 intent / answer_phase，仅正文 delta */
  plainSecondTurn?: boolean;
  conversationId?: string;
};

function splitSseBody(body: string): string[] {
  return body.split(/(?=id: \d+\n)/).filter(Boolean);
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function startSlowSseServer(
  bodyForTurn: (postCount: number) => string,
  delayForTurn: (postCount: number) => number,
  conversationId: string,
): Promise<{ server: Server; url: string }> {
  let postCount = 0;
  const server = createServer(async (req, res) => {
    if (req.method !== 'POST' || req.url !== '/chat/stream') {
      res.writeHead(404);
      res.end();
      return;
    }
    postCount += 1;
    const currentPost = postCount;
    const body = bodyForTurn(currentPost);
    const slowMs = delayForTurn(currentPost);
    res.writeHead(200, {
      'access-control-allow-origin': '*',
      'cache-control': 'no-cache',
      'content-type': 'text/event-stream; charset=utf-8',
      'x-conversation-id': conversationId,
      'x-stream-job-id': String(currentPost),
    });
    for (const part of splitSseBody(body)) {
      res.write(part);
      if (slowMs > 0) {
        await delay(slowMs);
      }
    }
    res.end();
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const addr = server.address();
  if (!addr || typeof addr === 'string') {
    server.close();
    throw new Error('slow SSE server did not bind a TCP port');
  }
  return { server, url: `http://127.0.0.1:${addr.port}/chat/stream` };
}

/**
 * 按 POST 顺序：首轮问候、次轮技能；次轮可选慢速 body 以复现「生成中途停住」。
 */
export async function installTwoTurnChatStreamStub(
  page: Page,
  opts: TwoTurnStreamStubOptions = {},
): Promise<void> {
  const conversationId = opts.conversationId ?? TWO_TURN_CONV_ID;
  let postCount = 0;
  const bodyForTurn = (count: number) =>
    count >= 2
      ? opts.plainSecondTurn
        ? buildSkillsTurnStreamBodyPlainOnly()
        : buildSkillsTurnStreamBody()
      : buildGreetingTurnStreamBody();
  const delayForTurn = (count: number) =>
    count >= 2 ? (opts.slowSecondTurnMs ?? 0) : (opts.slowFirstTurnMs ?? 0);

  if ((opts.slowFirstTurnMs ?? 0) > 0 || (opts.slowSecondTurnMs ?? 0) > 0) {
    const slow = await startSlowSseServer(bodyForTurn, delayForTurn, conversationId);
    page.once('close', () => slow.server.close());
    await page.route('**/chat/stream', async (route: Route) => {
      if (route.request().method() !== 'POST') {
        await route.continue();
        return;
      }
      await route.continue({ url: slow.url });
    });
    return;
  }

  await page.route('**/chat/stream', async (route: Route) => {
    if (route.request().method() !== 'POST') {
      await route.continue();
      return;
    }
    postCount += 1;
    const body = bodyForTurn(postCount);

    await route.fulfill({
      status: 200,
      headers: {
        'content-type': 'text/event-stream; charset=utf-8',
        'cache-control': 'no-cache',
        'x-conversation-id': conversationId,
        'x-stream-job-id': String(postCount),
      },
      body,
    });
  });
}

/** 水合 GET：服务端快照常不含 intent 行，刻意只返 user + assistant 以压 merge 边界。 */
export async function installTwoTurnHydrateStub(
  page: Page,
  conversationId = TWO_TURN_CONV_ID,
): Promise<void> {
  await page.route('**/conversation/messages**', async (route) => {
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
        revision: 2,
        total_count: 2,
        window_start_index: 0,
        has_older: false,
        messages: [
          { role: 'user', content: '你好' },
          { role: 'assistant', content: '你好！我是 CrabMate 助手。' },
        ],
      }),
    });
  });
}

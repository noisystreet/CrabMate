/**
 * Phase 5 单一读路径：预置 StoredMessage 行与导出断言辅助。
 */
import { expect, type APIRequestContext, type Page } from '@playwright/test';
import * as fs from 'fs';

import { openSessionListModal } from './sidebar';
import { putWorkspaceSessions } from './session-prefs';
import { visibleChatLayer } from './assertions';

export const PHASE5_SESSION_ID = 's_e2e_phase5';

export const PHASE5_USER_PROMPT = '分析当前目录';

/** 较长排版（保留首条）；与 compact 构成 fuzzy duplicate。 */
export const PHASE5_LISTING_ANSWER =
  '当前目录下有三个压缩包：\n\n1. **A** — x';

/** 较短排版；读路径应折叠，不在 UI/导出中单独出现。 */
export const PHASE5_COMPACT_ANSWER = '当前目录下有三个压缩包：\n1. **A** — x';

export const PHASE5_SINGLE_ANSWER = '当前目录下有三个压缩包。';

export type E2eStoredMessage = {
  id: string;
  role: string;
  text: string;
  reasoning_text?: string;
  state?: string;
  is_tool?: boolean;
  tool_call_id?: string | null;
  tool_name?: string | null;
  created_at?: number;
};

export function e2eUserMessage(id: string, text: string): E2eStoredMessage {
  return { id, role: 'user', text };
}

export function e2eAssistantMessage(id: string, text: string, opts?: { state?: string }): E2eStoredMessage {
  return {
    id,
    role: 'assistant',
    text,
    reasoning_text: '',
    ...(opts?.state ? { state: opts.state } : {}),
  };
}

export function e2eCommentaryBeforeTools(id: string, reasoning: string): E2eStoredMessage {
  return {
    id,
    role: 'assistant',
    text: '',
    reasoning_text: reasoning,
    state: 'commentary_before_tools',
  };
}

export function e2eFinalResponseSnapshot(id: string, text: string): E2eStoredMessage {
  return {
    id,
    role: 'assistant',
    text,
    reasoning_text: '',
    state: JSON.stringify({ k: 'cm_tl', t: 'final_response_snapshot' }),
  };
}

export function e2eOrchestrationRoute(id: string): E2eStoredMessage {
  return {
    id,
    role: 'assistant',
    text: '### CrabMate·staged_timeline\n编排路由：freeform\n{}',
    reasoning_text: '',
  };
}

export async function putPhase5Session(
  request: APIRequestContext,
  messages: E2eStoredMessage[],
  sessionId = PHASE5_SESSION_ID,
): Promise<void> {
  await putWorkspaceSessions(
    request,
    [
      {
        id: sessionId,
        title: `E2E Phase5 ${sessionId}`,
        draft: '',
        messages,
        updated_at: Date.now(),
        pinned: false,
        starred: false,
      },
    ],
    sessionId,
  );
}

export function visibleAssistantRows(page: Page) {
  // `.msg-meta-role` 与 `data-testid=chat-message-row` 同属 `.msg-stack` 子节点（兄弟关系）。
  return visibleChatLayer(page)
    .locator('.msg-stack')
    .filter({
      has: page.locator('.msg-meta-role').filter({ hasText: /^助手$|^Assistant$/ }),
    });
}

async function readDownloadText(download: import('@playwright/test').Download): Promise<string> {
  const path = await download.path();
  if (path) {
    return fs.readFileSync(path, 'utf-8');
  }
  const stream = await download.createReadStream();
  if (!stream) {
    throw new Error('export download has no path or stream');
  }
  const chunks: Buffer[] = [];
  for await (const chunk of stream) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  }
  return Buffer.concat(chunks).toString('utf-8');
}

function sessionModalRow(page: Page, sessionId: string) {
  return page.locator('.session-row').filter({
    has: page.getByTestId(`session-modal-open-${sessionId}`),
  });
}

async function ensureSessionListModal(page: Page): Promise<void> {
  const modal = page.getByTestId('session-list-modal');
  if (await modal.isVisible()) {
    return;
  }
  await openSessionListModal(page);
}

export async function exportSessionJsonFromModal(
  page: Page,
  sessionId: string,
): Promise<{ messages: { role: string; content?: string | null }[] }> {
  await ensureSessionListModal(page);
  const downloadPromise = page.waitForEvent('download');
  await sessionModalRow(page, sessionId).getByRole('button', { name: 'JSON' }).click();
  const download = await downloadPromise;
  const raw = await readDownloadText(download);
  return JSON.parse(raw) as { messages: { role: string; content?: string | null }[] };
}

export async function exportSessionMarkdownFromModal(page: Page, sessionId: string): Promise<string> {
  await ensureSessionListModal(page);
  const downloadPromise = page.waitForEvent('download');
  await sessionModalRow(page, sessionId).getByRole('button', { name: 'MD' }).click();
  const download = await downloadPromise;
  return readDownloadText(download);
}

export function countMarkdownAssistantSections(md: string): number {
  const matches = md.match(/^## 助手$|^## Assistant$/gm);
  return matches?.length ?? 0;
}

export function assistantMessagesInExport(file: { messages: { role: string }[] }) {
  return file.messages.filter((m) => m.role === 'assistant');
}

/** 打开页面并等待 Phase5 会话首屏渲染。 */
export async function gotoPhase5Session(page: Page): Promise<void> {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();
  await expect(page.getByText(PHASE5_USER_PROMPT)).toBeVisible();
}

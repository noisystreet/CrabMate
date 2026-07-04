import { expect, test } from '@playwright/test';

import {
  countMarkdownAssistantSections,
  exportSessionMarkdownFromModal,
  fillComposerDraft,
  putFreshLocalSession,
  UI_TIMEOUT,
} from './helpers';

const REAL_LLM_ENABLED = process.env.REAL_LLM_E2E === '1';
const REAL_LLM_TIMEOUT = 300_000;
const SESSION_ID = 's_e2e_real_turn_layout';
const WORKSPACE = process.env.REAL_LLM_WORKSPACE || '/home/gzz/test';

async function sendAndWaitForStream(page: import('@playwright/test').Page, text: string): Promise<void> {
  const streamDone = page.waitForResponse(
    (res) => res.url().includes('/chat/stream') && res.request().method() === 'POST',
    { timeout: REAL_LLM_TIMEOUT },
  );
  await fillComposerDraft(page, text);
  await page.getByTestId('chat-send-button').click();
  const response = await streamDone;
  expect(response.ok(), await response.text()).toBeTruthy();
  await expect(page.getByTestId('chat-send-button')).toBeEnabled({ timeout: REAL_LLM_TIMEOUT });
  await expect(page.getByRole('button', { name: '停止' })).toBeDisabled({ timeout: REAL_LLM_TIMEOUT });
}

test.describe('real LLM turn layout (DeepSeek multi-turn + export)', () => {
  test.skip(!REAL_LLM_ENABLED, 'set REAL_LLM_E2E=1 to run against the configured model backend');

  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, SESSION_ID, 'E2E real turn layout');
    const setWs = await request.post('/workspace', { data: { path: WORKSPACE } });
    expect(setWs.ok(), await setWs.text()).toBeTruthy();
  });

  test('two turns: analyze dir then build hpcg — export has batch + final, not mega bubble', async ({
    page,
  }) => {
    test.setTimeout(REAL_LLM_TIMEOUT * 2 + 60_000);

    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible({
      timeout: UI_TIMEOUT,
    });

    await sendAndWaitForStream(page, '分析当前目录');
    await sendAndWaitForStream(page, '编译 hpcg');

    const md = await exportSessionMarkdownFromModal(page, SESSION_ID);
    const assistantCount = countMarkdownAssistantSections(md);
    expect(assistantCount).toBeGreaterThanOrEqual(2);

    // 第二轮「编译 hpcg」：batch 在工具前、终答在工具后（勿巨泡合一）。
    const compileTurnStart = md.search(/## 用户\n\n[^\n]*编译[^\n]*hpcg/i);
    expect(compileTurnStart).toBeGreaterThanOrEqual(0);
    const turnSlice = md.slice(compileTurnStart);
    const toolIdxInTurn = turnSlice.search(/^## 工具\n\n/m);
    expect(toolIdxInTurn).toBeGreaterThanOrEqual(0);

    const beforeTools = turnSlice.slice(0, toolIdxInTurn);
    const afterTools = turnSlice.slice(toolIdxInTurn);
    expect(beforeTools).toMatch(/## 助手\n\n[\s\S]{12,}/);
    expect(afterTools).toMatch(/## 助手\n\n[\s\S]{4,}/);
    expect(afterTools).toMatch(/编译完成|xhpcg|HPCG|总结|完成/i);

    const beforeToolAssistants = [
      ...beforeTools.matchAll(/## 助手\n\n([\s\S]*?)(?=\n## |$)/g),
    ].map((m) => m[1].trim());
    const afterToolAssistants = [
      ...afterTools.matchAll(/## 助手\n\n([\s\S]*?)(?=\n## |$)/g),
    ].map((m) => m[1].trim());
    expect(beforeToolAssistants.length).toBeGreaterThanOrEqual(1);
    expect(beforeToolAssistants[0].length).toBeGreaterThan(10);
    expect(afterToolAssistants.length).toBeGreaterThanOrEqual(1);
    expect(afterToolAssistants[0]).toMatch(/编译|xhpcg|HPCG|总结|完成|成功|make/i);
    expect(beforeToolAssistants[0]).not.toBe(afterToolAssistants[0]);
  });
});

import { expect, test } from '@playwright/test';

import {
  REAL_LLM_ENABLED,
  REAL_LLM_SESSION_ANALYZE,
  REAL_LLM_TIMEOUT,
  REAL_LLM_WORKSPACE,
  countMarkdownAssistantSections,
  createRealLlmArtifactHooks,
  exportSessionArtifacts,
  gotoCrabMateHome,
  putFreshLocalSession,
  sendAndWaitForStream,
  setupRealLlmWorkspace,
} from './helpers';

const { state: artifacts, afterEach: artifactAfterEach } =
  createRealLlmArtifactHooks(REAL_LLM_SESSION_ANALYZE);

test.describe('real LLM turn layout — analyze only', () => {
  test.skip(!REAL_LLM_ENABLED, 'set REAL_LLM_E2E=1 to run against the configured model backend');

  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, REAL_LLM_SESSION_ANALYZE, 'E2E real analyze');
    artifacts.workspaceHints = await setupRealLlmWorkspace(request, REAL_LLM_WORKSPACE);
  });

  test.afterEach(async ({ request }, testInfo) => {
    await artifactAfterEach(request, testInfo);
  });

  test('single turn: 分析当前目录 — streams and export has assistant reply', async ({ page }) => {
    test.setTimeout(REAL_LLM_TIMEOUT + 60_000);

    await gotoCrabMateHome(page);
    await sendAndWaitForStream(page, '分析当前目录');

    const { md, json } = await exportSessionArtifacts(page, REAL_LLM_SESSION_ANALYZE);
    artifacts.exportMd = md;
    artifacts.exportJson = json;
    expect(countMarkdownAssistantSections(md)).toBeGreaterThanOrEqual(1);
    expect(md).toMatch(/## 用户\n\n分析当前目录/);
    await expect(page.getByTestId('chat-messages-scroller')).not.toContainText(/对话失败|请求失败/);
  });
});

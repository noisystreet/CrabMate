import { expect, test } from '@playwright/test';

import {
  REAL_LLM_ENABLED,
  REAL_LLM_SESSION_FULL,
  REAL_LLM_TIMEOUT,
  REAL_LLM_WORKSPACE,
  assertCompileTurnLayoutExport,
  countMarkdownAssistantSections,
  createRealLlmArtifactHooks,
  exportSessionArtifacts,
  gotoCrabMateHome,
  putFreshLocalSession,
  sendAndWaitForStreamWithLayoutMonitor,
  setupRealLlmWorkspace,
} from './helpers';

const { state: artifacts, afterEach: artifactAfterEach } =
  createRealLlmArtifactHooks(REAL_LLM_SESSION_FULL);

test.describe('real LLM turn layout (DeepSeek multi-turn + export)', () => {
  test.skip(!REAL_LLM_ENABLED, 'set REAL_LLM_E2E=1 to run against the configured model backend');

  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, REAL_LLM_SESSION_FULL, 'E2E real turn layout');
    artifacts.workspaceHints = await setupRealLlmWorkspace(request, REAL_LLM_WORKSPACE);
  });

  test.afterEach(async ({ request }, testInfo) => {
    await artifactAfterEach(request, testInfo);
  });

  test('two turns: analyze dir then build hpcg — export has batch + final, not mega bubble', async ({
    page,
  }) => {
    test.setTimeout(REAL_LLM_TIMEOUT * 2 + 60_000);

    await gotoCrabMateHome(page);
    await sendAndWaitForStreamWithLayoutMonitor(page, '分析当前目录', { enabled: false });
    artifacts.streamLayoutReport =
      (await sendAndWaitForStreamWithLayoutMonitor(page, '编译 hpcg')) ?? undefined;

    const { md, json } = await exportSessionArtifacts(page, REAL_LLM_SESSION_FULL);
    artifacts.exportMd = md;
    artifacts.exportJson = json;
    artifacts.compileReport = assertCompileTurnLayoutExport(md);
    expect(countMarkdownAssistantSections(md)).toBeGreaterThanOrEqual(2);
  });
});

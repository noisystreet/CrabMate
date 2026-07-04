import { expect, test } from '@playwright/test';

import {
  REAL_LLM_ENABLED,
  REAL_LLM_SESSION_COMPILE,
  REAL_LLM_TIMEOUT,
  REAL_LLM_WORKSPACE,
  assertCompileTurnLayoutExport,
  createRealLlmArtifactHooks,
  exportSessionArtifacts,
  gotoCrabMateHome,
  putFreshLocalSession,
  sendAndWaitForStreamWithLayoutMonitor,
  setupRealLlmWorkspace,
} from './helpers';

const { state: artifacts, afterEach: artifactAfterEach } =
  createRealLlmArtifactHooks(REAL_LLM_SESSION_COMPILE);

test.describe('real LLM turn layout — compile hpcg only', () => {
  test.skip(!REAL_LLM_ENABLED, 'set REAL_LLM_E2E=1 to run against the configured model backend');

  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, REAL_LLM_SESSION_COMPILE, 'E2E real compile');
    artifacts.workspaceHints = await setupRealLlmWorkspace(request, REAL_LLM_WORKSPACE);
  });

  test.afterEach(async ({ request }, testInfo) => {
    await artifactAfterEach(request, testInfo);
  });

  test('single turn: 编译 hpcg — export batch + final, not mega bubble', async ({ page }) => {
    test.setTimeout(REAL_LLM_TIMEOUT + 60_000);

    await gotoCrabMateHome(page);
    artifacts.streamLayoutReport =
      (await sendAndWaitForStreamWithLayoutMonitor(page, '编译 hpcg')) ?? undefined;
    if (artifacts.streamLayoutReport && artifacts.streamLayoutReport.violations.length > 0) {
      console.warn(
        '[real-llm] stream layout violations:',
        JSON.stringify(artifacts.streamLayoutReport.violations),
      );
    }

    const { md, json } = await exportSessionArtifacts(page, REAL_LLM_SESSION_COMPILE);
    artifacts.exportMd = md;
    artifacts.exportJson = json;
    artifacts.compileReport = assertCompileTurnLayoutExport(md);
    expect(artifacts.compileReport.tool_section_found).toBe(true);
  });
});

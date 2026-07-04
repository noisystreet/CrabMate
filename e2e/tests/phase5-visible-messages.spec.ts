import { expect, test } from '@playwright/test';

import {
  PHASE5_COMPACT_ANSWER,
  PHASE5_LISTING_ANSWER,
  PHASE5_SINGLE_ANSWER,
  PHASE5_USER_PROMPT,
  assistantMessagesInExport,
  countMarkdownAssistantSections,
  e2eAssistantMessage,
  e2eCommentaryBeforeTools,
  e2eFinalResponseSnapshot,
  e2eOrchestrationRoute,
  e2eUserMessage,
  exportSessionJsonFromModal,
  exportSessionMarkdownFromModal,
  gotoPhase5Session,
  putPhase5Session,
  visibleAssistantRows,
} from './helpers';

test.describe('Phase 5 single read path (visible_messages)', () => {
  // 共享 CM_CRABMATE_USER_DATA_DIR；串行避免 PUT sessions / prefs 竞态。
  test.describe.configure({ mode: 'serial' });

  test('fuzzy duplicate assistant: chat and export show one answer', async ({ page, request }) => {
    const sessionId = 's_e2e_phase5_fuzzy';
    await putPhase5Session(
      request,
      [
        e2eUserMessage('u1', PHASE5_USER_PROMPT),
        e2eAssistantMessage('a1', PHASE5_LISTING_ANSWER),
        e2eAssistantMessage('a2', PHASE5_COMPACT_ANSWER),
      ],
      sessionId,
    );

    await gotoPhase5Session(page);
    await expect(visibleAssistantRows(page)).toHaveCount(1);
    await expect(page.getByText('当前目录下有三个压缩包')).toBeVisible();
    await expect(page.getByText(PHASE5_COMPACT_ANSWER)).not.toBeVisible();

    const json = await exportSessionJsonFromModal(page, sessionId);
    expect(assistantMessagesInExport(json)).toHaveLength(1);
    expect(assistantMessagesInExport(json)[0]?.content).toContain('当前目录下有三个压缩包');

    const md = await exportSessionMarkdownFromModal(page, sessionId);
    expect(countMarkdownAssistantSections(md)).toBe(1);
    expect(md.match(/当前目录下有三个压缩包/g)?.length).toBe(1);
  });

  test('duplicate final_response_snapshot: hidden in chat and export', async ({ page, request }) => {
    const sessionId = 's_e2e_phase5_snap';
    await putPhase5Session(
      request,
      [
        e2eUserMessage('u1', PHASE5_USER_PROMPT),
        e2eAssistantMessage('a1', PHASE5_SINGLE_ANSWER),
        e2eFinalResponseSnapshot('snap', PHASE5_SINGLE_ANSWER),
      ],
      sessionId,
    );

    await gotoPhase5Session(page);
    await expect(visibleAssistantRows(page)).toHaveCount(1);

    const json = await exportSessionJsonFromModal(page, sessionId);
    expect(assistantMessagesInExport(json)).toHaveLength(1);
    expect(assistantMessagesInExport(json)[0]?.content).toBe(PHASE5_SINGLE_ANSWER);
  });

  test('ephemeral commentary and orchestration route: only final answer visible', async ({
    page,
    request,
  }) => {
    const sessionId = 's_e2e_phase5_ephemeral';
    await putPhase5Session(
      request,
      [
        e2eUserMessage('u1', PHASE5_USER_PROMPT),
        e2eOrchestrationRoute('route'),
        e2eCommentaryBeforeTools('cmt', '我先列出目录结构'),
        e2eAssistantMessage('a1', PHASE5_SINGLE_ANSWER),
      ],
      sessionId,
    );

    await gotoPhase5Session(page);
    await expect(visibleAssistantRows(page)).toHaveCount(1);
    await expect(page.getByText(PHASE5_SINGLE_ANSWER)).toBeVisible();
    await expect(page.getByText('编排路由')).not.toBeVisible();
    await expect(page.getByText('我先列出目录结构')).not.toBeVisible();

    const json = await exportSessionJsonFromModal(page, sessionId);
    expect(assistantMessagesInExport(json)).toHaveLength(1);
    expect(json.messages.map((m) => m.role)).toEqual(['user', 'assistant']);
  });
});

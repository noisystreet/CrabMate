import { expect, test } from '@playwright/test';

import {
  UI_TIMEOUT,
  COMMENTARY_MAKE,
  COMMENTARY_READ,
  COMMENTARY_UNPACK,
  exportSessionMarkdownFromModal,
  FINAL_ANSWER,
  indexOfRowContaining,
  installTurnLayoutStreamStub,
  putFreshLocalSession,
  sendStubMessage,
  TOOL_MAKE_EXPORT,
  TOOL_MAKE_UI,
  TOOL_READ_UI,
  TOOL_UNPACK_UI,
  TURN_LAYOUT_SESSION_ID,
  TURN_LAYOUT_USER_PROMPT,
  visibleTimelineBlockTexts,
} from './helpers';

test.describe('SSE turn layout (post-tool interleaved commentary)', () => {
  // 共享 CM_CRABMATE_USER_DATA_DIR 与同一会话 id；串行避免 PUT sessions 竞态。
  test.describe.configure({ mode: 'serial' });

  test.beforeEach(async ({ request }) => {
    await putFreshLocalSession(request, TURN_LAYOUT_SESSION_ID, 'E2E turn layout');
  });

  test('multi-tool commentary appears before each tool in chat', async ({ page }) => {
    await installTurnLayoutStreamStub(page);
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await sendStubMessage(page, TURN_LAYOUT_USER_PROMPT);

    await expect(page.getByText(COMMENTARY_UNPACK)).toBeVisible({ timeout: UI_TIMEOUT });
    await expect(page.getByText(COMMENTARY_READ)).toBeVisible({ timeout: UI_TIMEOUT });
    await expect(page.getByText(COMMENTARY_MAKE)).toBeVisible({ timeout: UI_TIMEOUT });
    await expect(page.getByText(FINAL_ANSWER)).toBeVisible({ timeout: UI_TIMEOUT });

    const blocks = await visibleTimelineBlockTexts(page);
    const unpackCommentary = indexOfRowContaining(blocks, COMMENTARY_UNPACK);
    const unpackTool = indexOfRowContaining(blocks, TOOL_UNPACK_UI);
    const readCommentary = indexOfRowContaining(blocks, COMMENTARY_READ);
    const readTool = indexOfRowContaining(blocks, TOOL_READ_UI);
    const makeCommentary = indexOfRowContaining(blocks, COMMENTARY_MAKE);
    const makeTool = indexOfRowContaining(blocks, TOOL_MAKE_UI);

    expect(unpackCommentary).toBeGreaterThanOrEqual(0);
    expect(unpackTool).toBeGreaterThan(unpackCommentary);
    expect(readCommentary).toBeGreaterThanOrEqual(0);
    expect(readTool).toBeGreaterThan(readCommentary);
    expect(makeCommentary).toBeGreaterThanOrEqual(0);
    expect(makeTool).toBeGreaterThan(makeCommentary);
  });

  test('segment_end before tool_call keeps commentary visible through stream end', async ({ page }) => {
    await installTurnLayoutStreamStub(page, { segmentEndBeforeUnpackTool: true });
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await sendStubMessage(page, TURN_LAYOUT_USER_PROMPT);

    await expect(page.getByText(COMMENTARY_UNPACK)).toBeVisible({ timeout: UI_TIMEOUT });
    await expect(page.getByText(FINAL_ANSWER)).toBeVisible({ timeout: UI_TIMEOUT });
    await expect(page.getByText(COMMENTARY_READ)).toBeVisible({ timeout: UI_TIMEOUT });
  });

  test('markdown export interleaves commentary before tools', async ({ page }) => {
    await installTurnLayoutStreamStub(page);
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible();

    await sendStubMessage(page, TURN_LAYOUT_USER_PROMPT);
    await expect(page.getByText(FINAL_ANSWER)).toBeVisible({ timeout: UI_TIMEOUT });

    const md = await exportSessionMarkdownFromModal(page, TURN_LAYOUT_SESSION_ID);
    const idxUnpackCommentary = md.indexOf(COMMENTARY_UNPACK);
    const idxUnpackTool = md.indexOf(TOOL_UNPACK_UI);
    const idxReadCommentary = md.indexOf(COMMENTARY_READ);
    const idxReadTool = md.indexOf(TOOL_READ_UI);
    const idxMakeCommentary = md.indexOf(COMMENTARY_MAKE);
    const idxMakeTool = md.indexOf(TOOL_MAKE_EXPORT);

    expect(idxUnpackCommentary).toBeGreaterThanOrEqual(0);
    expect(idxUnpackTool).toBeGreaterThan(idxUnpackCommentary);
    expect(idxReadCommentary).toBeGreaterThan(idxUnpackTool);
    expect(idxReadTool).toBeGreaterThan(idxReadCommentary);
    expect(idxMakeCommentary).toBeGreaterThan(idxReadTool);
    expect(idxMakeTool).toBeGreaterThan(idxMakeCommentary);
    expect(md).toContain(FINAL_ANSWER);
  });
});

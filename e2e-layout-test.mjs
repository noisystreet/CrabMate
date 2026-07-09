// E2E 布局测试：通过 Playwright headless 浏览器验证 data-testid 存在性。
// 用法: cd /tmp/playwright-test && node /path/to/e2e-layout-test.mjs

import { chromium } from 'playwright';
import assert from 'node:assert';

const BASE = 'http://127.0.0.1:9191';

// 同 Victauri 测试：先注入会话，再加载页面
async function setupSession(page) {
  const resp = await page.request.put(`${BASE}/user-data/prefs`, {
    data: { locale: 'zh', theme: 'light', side_panel_view: 'hidden', side_width: 280, editor_layout_mode: false },
  });
  assert.ok(resp.ok(), 'prefs PUT should succeed');
  const sid = 's_e2e_layout';
  const sessionsResp = await page.request.put(`${BASE}/user-data/workspaces/current/sessions`, {
    data: { sessions: [{ id: sid, title: 'Layout E2E', draft: '', messages: [], updated_at: 1, pinned: false, starred: false }], active_session_id: sid },
  });
  assert.ok(sessionsResp.ok(), 'sessions PUT should succeed');
}

async function main() {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();

  // 1. 设置会话
  await page.goto(BASE, { waitUntil: 'networkidle', timeout: 15000 });
  await setupSession(page);

  // 2. 重新加载，让前端读到已存在的会话
  await page.reload({ waitUntil: 'networkidle', timeout: 15000 });

  // 3. 等待 WASM 渲染完成
  await page.waitForSelector('[data-testid="status-bar"]', { timeout: 20000 });
  console.log('✓ App loaded, status-bar visible');

  // ====== 测试 1：核心布局区域 ======
  const chatColumn = await page.isVisible('[data-testid="chat-column"]');
  assert.ok(chatColumn, 'chat-column should be visible');
  console.log('✓ chat-column visible');

  const sidePanel = await page.isVisible('[data-testid="side-panel"]');
  assert.ok(!sidePanel, 'side-panel should be hidden by default');
  console.log('✓ side-panel hidden by default');

  const msgScroller = await page.isVisible('[data-testid="chat-messages-scroller"]');
  assert.ok(msgScroller, 'chat-messages-scroller should be visible');
  console.log('✓ chat-messages-scroller visible');

  // ====== 测试 2：输入栏 ======
  const composerInput = await page.isVisible('[data-testid="chat-composer-input"]');
  assert.ok(composerInput, 'composer input should be visible');
  console.log('✓ chat-composer-input visible');

  const sendBtn = await page.isVisible('[data-testid="chat-send-button"]');
  assert.ok(sendBtn, 'send button should be visible');
  console.log('✓ chat-send-button visible');

  // ====== 测试 3：IDE 布局隐藏 ======
  const ideRoot = await page.isVisible('[data-testid="ide-layout-root"]');
  assert.ok(!ideRoot, 'IDE layout should be hidden in chat mode');
  console.log('✓ ide-layout-root hidden in chat mode');

  // ====== 测试 4：审批栏存在 ======
  const approvalBars = await page.locator('[data-testid="approval-bar"]').count();
  assert.strictEqual(approvalBars, 1, 'exactly one approval bar');
  console.log('✓ approval-bar exists');

  // ====== 测试 5：消息气泡排列顺序 ======
  const msgRows = await page.locator('[data-testid="chat-message-row"]').count();
  assert.strictEqual(msgRows, 0, 'no message rows on empty session');
  console.log('✓ no message rows on empty session (correct)');

  // ====== 测试 6：工作区面板侧栏切换 ======
  // 检查 workspace panel 默认存在（side_panel_view 决定）
  const workspacePanel = await page.locator('[data-testid="workspace-panel"]').count();
  // 默认 hidden，所以不可见
  const wpVisible = await page.isVisible('[data-testid="workspace-panel"]');
  assert.ok(!wpVisible, 'workspace-panel should be hidden when side panel is hidden');
  console.log('✓ workspace-panel respects side_panel_view=hidden');

  await browser.close();
  console.log('\n✅ All UI layout tests passed!');
}

main().catch(err => {
  console.error('\n❌ Test failed:', err.message);
  process.exit(1);
});

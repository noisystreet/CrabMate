/**
 * Mock SSE 回归测试：命令审批场景
 *
 * 使用 AG-UI（V2）协议格式 mock SSE，覆盖命令审批相关的前端渲染流程：
 *   1. 审批弹窗出现 → 允许一次 → 工具继续执行 → 终答
 *   2. 审批弹窗出现 → 拒绝 → 工具不执行 → 终答
 *
 * 运行方式（前置：`cargo run -- serve` 在 127.0.0.1:8080 运行）：
 *   cd e2e && npx playwright test specs/mock-approval-scenarios.spec.ts
 */

import { test, expect, Page, Route } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const BASE_SID = "s_e2e_mock_approval";

/**
 * 安装 mock SSE 响应，并在响应头中包含 approval-session-id。
 * 同时拦截 /chat/approval POST 以防止真实后端因无对应 session 报错。
 */
function installMockSseWithApproval(
  page: Page,
  sseBody: string,
  convId = "e2e-conv-approval",
) {
  // 拦截 /chat/stream 返回 mock SSE，含 x-approval-session-id 头
  void page.route("**/chat/stream", (route: Route) => {
    if (route.request().method() !== "POST") {
      return route.continue();
    }
    return route.fulfill({
      status: 200,
      headers: {
        "content-type": "text/event-stream; charset=utf-8",
        "x-conversation-id": convId,
        "x-stream-job-id": "1",
        "x-approval-session-id": "mock-appr-session-001",
      },
      body: sseBody,
    });
  });

  // 拦截 /chat/approval POST 返回 204（模拟后端接受的审批决策）
  void page.route("**/chat/approval", (route: Route) => {
    if (route.request().method() !== "POST") {
      return route.continue();
    }
    return route.fulfill({ status: 204 });
  });
}

test.describe("命令审批场景回归", () => {
  // ---------------------------------------------------------------------------
  // 用例 1：审批弹窗 → 允许一次 → 工具执行 → 终答可见
  // ---------------------------------------------------------------------------
  test("approval_allow_once_then_tool_executes", async ({ page }) => {
    const answer = "当前目录有 3 个文件。";
    // SSE 序列：command_approval → 工具调用 → 工具结果 → 终答
    const sse = [
      // 1. answer_phase（终答阶段开始）
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      // 2. reasoning delta
      "id: 2\ndata: 我来列出目录内容。\n\n",
      // 3. 命令审批请求（V2 AG-UI 格式：CUSTOM + customType）
      'id: 3\ndata: {"type":"CUSTOM","customType":"command_approval","data":{"command":"ls","args":"-la","allowlistKey":"ls"}}\n\n',
      // 4. 工具调用声明
      'id: 4\ndata: {"type":"TOOL_CALL_START","toolCallId":"tc-appr-1","name":"run_command"}\ndata: {"type":"TOOL_CALL_ARGS","toolCallId":"tc-appr-1","args":"{\\"command\\":\\"ls\\",\\"args\\":[\\"-la\\"]}"}\ndata: {"type":"TOOL_CALL_END","toolCallId":"tc-appr-1"}\n\n',
      // 5. tool_running
      'id: 5\ndata: {"type":"CUSTOM","customType":"tool_running","data":{"running":true}}\n\n',
      // 6. 工具结果
      'id: 6\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"tc-appr-1","content":"drwxr-xr-x  .\\ndrwxr-xr-x  ..\\ndrwxr-xr-x  src","metadata":{"name":"run_command","ok":true,"summary":"ls -la 执行成功"}}\n\n',
      // 7. turn_tool_phase_end
      'id: 7\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
      // 8. 终答阶段（工具后）
      'id: 8\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      // 9. 终答 delta
      `id: 9\ndata: ${answer}\n\n`,
      // 10. 流结束
      'id: 10\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");

    await installMockSseWithApproval(page, sse);
    await seedSession(page, BASE_SID + "_allow_once");
    await sendMessage(page, "列出当前目录文件");

    // 等待审批弹窗出现
    await expect(page.locator('[data-testid="approval-modal"]')).toBeVisible({
      timeout: 10_000,
    });

    // 审批弹窗应显示命令预览
    await expect(
      page.locator('[data-testid="approval-modal"] .approval-modal-command'),
    ).toContainText("ls", { timeout: 3_000 });

    // 点击「允许一次」
    await page.locator('[data-testid="approval-allow-once"]').click();

    // 弹窗应消失
    await expect(
      page.locator('[data-testid="approval-modal"]'),
    ).not.toBeVisible({ timeout: 5_000 });

    // 等待就绪
    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25_000 },
    );

    // 终答内容可见
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(answer, { timeout: 5_000 });

    // TUI transcript：工具回合为 section.chat-tui-turn--tool
    await expect(page.locator("section.chat-tui-turn--tool")).toHaveCount(1);
  });

  // ---------------------------------------------------------------------------
  // 用例 2：审批弹窗 → 始终允许 → 工具执行 → 终答
  // ---------------------------------------------------------------------------
  test("approval_allow_always_then_tool_executes", async ({ page }) => {
    const answer = "磁盘使用情况：已用 50%。";
    const sse = [
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      "id: 2\ndata: 检查磁盘空间。\n\n",
      'id: 3\ndata: {"type":"CUSTOM","customType":"command_approval","data":{"command":"df","args":"-h","allowlistKey":"df"}}\n\n',
      'id: 4\ndata: {"type":"TOOL_CALL_START","toolCallId":"tc-appr-3","name":"run_command"}\ndata: {"type":"TOOL_CALL_ARGS","toolCallId":"tc-appr-3","args":"{\\"command\\":\\"df\\",\\"args\\":[\\"-h\\"]}"}\ndata: {"type":"TOOL_CALL_END","toolCallId":"tc-appr-3"}\n\n',
      'id: 5\ndata: {"type":"CUSTOM","customType":"tool_running","data":{"running":true}}\n\n',
      'id: 6\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"tc-appr-3","content":"Filesystem  Size  Used Avail Use% Mounted on\\n/dev/sda1  100G   50G   51G  50%  /","metadata":{"name":"run_command","ok":true,"summary":"df -h 执行成功"}}\n\n',
      'id: 7\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
      'id: 8\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      `id: 9\ndata: ${answer}\n\n`,
      'id: 10\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");

    await installMockSseWithApproval(page, sse);
    await seedSession(page, BASE_SID + "_allow_always");
    await sendMessage(page, "检查磁盘空间");

    // 等待审批弹窗出现
    await expect(page.locator('[data-testid="approval-modal"]')).toBeVisible({
      timeout: 10_000,
    });

    // 点击「始终允许」
    await page.locator('[data-testid="approval-allow-always"]').click();

    // 弹窗应消失
    await expect(
      page.locator('[data-testid="approval-modal"]'),
    ).not.toBeVisible({ timeout: 5_000 });

    // 等待就绪
    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25_000 },
    );

    // 终答内容可见
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(answer, { timeout: 5_000 });

    await expect(page.locator("section.chat-tui-turn--tool")).toHaveCount(1);
  });
});

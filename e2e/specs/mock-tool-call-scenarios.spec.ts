/**
 * Mock SSE 回归测试：工具调用场景
 *
 * 使用 AG-UI（V2）协议格式 mock SSE，覆盖工具调用相关的前端渲染流程：
 *   1. 单工具调用 → 工具卡渲染 + 终答
 *   2. 多工具串联 → 多工具卡 + commentary + 终答
 *   3. 工具执行失败 → 工具卡错误状态
 *
 * 运行方式（前置：`cargo run -- serve` 在 127.0.0.1:8080 运行）：
 *   cd e2e && npx playwright test
 */

import { test, expect } from "@playwright/test";
import { seedSession, sendMessage, installMockSse } from "../fixtures/helpers";

const BASE_SID = "s_e2e_mock_tool";

test.describe("工具调用场景回归", () => {
  // ---------------------------------------------------------------------------
  // 用例 1：单工具调用 — commentary + 工具卡 + 终答
  // ---------------------------------------------------------------------------
  test("single_tool_call_with_answer", async ({ page }) => {
    const postToolAnswer =
      "文件内容如下：main.rs 是入口文件，其中包含主函数和基本配置。";
    const sse = [
      // 1. 终答阶段开始
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      // 2. reasoning delta
      "id: 2\ndata: 我来分析一下你的问题。\n\n",
      // 3. turn_segment_start: commentary 段（beforeToolCallId）
      'id: 3\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-before-tc1","kind":"commentary","beforeToolCallId":"tc-1"}}\n\n',
      // 4. commentary delta
      "id: 4\ndata: 让我先读取文件内容。\n\n",
      // 5. turn_segment_end
      'id: 5\ndata: {"type":"CUSTOM","customType":"turn_segment_end","data":{"segmentId":"seg-before-tc1"}}\n\n',
      // 6-8. 工具调用声明（3 data: 行在同一块中，模拟后端 V2Encoder 行为）
      'id: 6\ndata: {"type":"TOOL_CALL_START","toolCallId":"tc-1","name":"read_file"}\ndata: {"type":"TOOL_CALL_ARGS","toolCallId":"tc-1","args":"{\\"path\\":\\"src/main.rs\\"}"}\ndata: {"type":"TOOL_CALL_END","toolCallId":"tc-1"}\n\n',
      // 9. tool_running
      'id: 7\ndata: {"type":"CUSTOM","customType":"tool_running","data":{"running":true}}\n\n',
      // 10. 工具结果
      'id: 8\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"tc-1","content":"fn main() {\\n    println!(\\"Hello\\");\\n}","metadata":{"name":"read_file","ok":true,"summary":"读取成功"}}\n\n',
      // 11. turn_tool_phase_end
      'id: 9\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
      // 12. 终答阶段（工具后）
      'id: 10\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      // 13. 终答 delta
      `id: 11\ndata: ${postToolAnswer}\n\n`,
      // 14. 流结束
      'id: 12\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");

    await installMockSse(page, sse);
    await seedSession(page, BASE_SID + "_1");
    await sendMessage(page, "读取 src/main.rs");

    // 等待就绪
    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );

    // 终答内容可见
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(postToolAnswer, { timeout: 5000 });

    // 应有 1 个工具卡
    const toolCards = await page
      .locator('[data-testid="chat-tool-card"]')
      .count();
    expect(toolCards).toBe(1);
  });

  // ---------------------------------------------------------------------------
  // 用例 2：多工具调用 — 2 个工具串联 + commentary + 终答
  // ---------------------------------------------------------------------------
  test("multiple_tool_calls_with_answer", async ({ page }) => {
    const answer = "项目包含 src/main.rs 和 src/lib.rs 两个文件。";
    const sse = [
      // answer_phase
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      "id: 2\ndata: 我来检查项目结构。\n\n",
      // 第一个工具前的 commentary
      'id: 3\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-before-tc1","kind":"commentary","beforeToolCallId":"tc-1"}}\n\n',
      "id: 4\ndata: 先列出目录。\n\n",
      'id: 5\ndata: {"type":"CUSTOM","customType":"turn_segment_end","data":{"segmentId":"seg-before-tc1"}}\n\n',
      // 第一个工具：list_dir
      'id: 6\ndata: {"type":"TOOL_CALL_START","toolCallId":"tc-1","name":"list_dir"}\ndata: {"type":"TOOL_CALL_ARGS","toolCallId":"tc-1"}\ndata: {"type":"TOOL_CALL_END","toolCallId":"tc-1"}\n\n',
      'id: 7\ndata: {"type":"CUSTOM","customType":"tool_running","data":{"running":true}}\n\n',
      'id: 8\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"tc-1","content":"src/","metadata":{"name":"list_dir","ok":true}}\n\n',
      // 第二个工具前的 commentary
      'id: 9\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-before-tc2","kind":"commentary","beforeToolCallId":"tc-2"}}\n\n',
      "id: 10\ndata: 再读取文件内容。\n\n",
      'id: 11\ndata: {"type":"CUSTOM","customType":"turn_segment_end","data":{"segmentId":"seg-before-tc2"}}\n\n',
      // 第二个工具：read_file
      'id: 12\ndata: {"type":"TOOL_CALL_START","toolCallId":"tc-2","name":"read_file"}\ndata: {"type":"TOOL_CALL_ARGS","toolCallId":"tc-2"}\ndata: {"type":"TOOL_CALL_END","toolCallId":"tc-2"}\n\n',
      'id: 13\ndata: {"type":"CUSTOM","customType":"tool_running","data":{"running":true}}\n\n',
      'id: 14\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"tc-2","content":"pub fn helper() {}","metadata":{"name":"read_file","ok":true}}\n\n',
      // turn_tool_phase_end
      'id: 15\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
      // 终答阶段
      'id: 16\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      `id: 17\ndata: ${answer}\n\n`,
      'id: 18\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");

    await installMockSse(page, sse);
    await seedSession(page, BASE_SID + "_2");
    await sendMessage(page, "检查项目结构");

    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );

    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(answer, { timeout: 5000 });

    // 应有 2 个工具卡
    const toolCards = await page
      .locator('[data-testid="chat-tool-card"]')
      .count();
    expect(toolCards).toBe(2);
  });

  // ---------------------------------------------------------------------------
  // 用例 3：工具执行失败 — 工具卡错误状态 + 终答
  // ---------------------------------------------------------------------------
  test("tool_call_error_shows_failure", async ({ page }) => {
    const answer = "文件读取失败，请检查路径是否正确。";
    const sse = [
      'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      "id: 2\ndata: 我尝试读取文件。\n\n",
      // 工具调用
      'id: 3\ndata: {"type":"TOOL_CALL_START","toolCallId":"tc-1","name":"read_file"}\ndata: {"type":"TOOL_CALL_ARGS","toolCallId":"tc-1","args":"{\\"path\\":\\"/nonexistent\\"}"}\ndata: {"type":"TOOL_CALL_END","toolCallId":"tc-1"}\n\n',
      'id: 4\ndata: {"type":"CUSTOM","customType":"tool_running","data":{"running":true}}\n\n',
      // 工具失败结果
      'id: 5\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"tc-1","content":"No such file or directory","metadata":{"name":"read_file","ok":false,"errorCode":"read_file_failed","summary":"文件不存在"}}\n\n',
      // turn_tool_phase_end
      'id: 6\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
      // 终答
      'id: 7\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
      `id: 8\ndata: ${answer}\n\n`,
      'id: 9\ndata: {"type":"RUN_FINISHED"}\n\n',
    ].join("");

    await installMockSse(page, sse);
    await seedSession(page, BASE_SID + "_3");
    await sendMessage(page, "读取不存在的文件");

    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 25000 },
    );

    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(answer, { timeout: 5000 });

    // 应有 1 个工具卡
    const toolCards = await page
      .locator('[data-testid="chat-tool-card"]')
      .count();
    expect(toolCards).toBe(1);
  });
});

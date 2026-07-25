/**
 * Mock SSE 回归测试：流式结束后多条助手正文不应合并为一条 stored_message。
 *
 * Bug：后端在单流内发送多轮 assistant_answer_phase + delta，前端流式处理
 * 将所有 delta 累积到同一条 stored_message，导致 DOM 层面所有助手文本挤在
 * 一个气泡中。重启后重新加载才正确拆分。
 *
 * SSE 事件序列模拟真实后端（来自 turn-replay-events.jsonl 分析）：
 *   round1: assistant_answer_phase → delta("第一段说明") → 工具
 *   round2: turn_segment_start(kind=answer) → assistant_answer_phase → delta("第二段说明") → 工具
 *   round3: turn_segment_start(kind=answer) → assistant_answer_phase → delta("第三段说明") → 工具
 *   round4: turn_segment_start(kind=answer) → assistant_answer_phase → delta("终答")
 *   RUN_FINISHED
 *
 * Bug 特征（流式结束后 DOM）：
 *   - 只有 2 行非工具消息：用户行 + 合并的助手气泡（所有正文拼接）
 *   - 而期待：用户行 + 4 个独立助手气泡 = 5 行
 */

import { test, expect } from "@playwright/test";
import { seedSession, sendMessage, installMockSse } from "../fixtures/helpers";

// 四轮助手正文的关键片段，用于检测是否分布在多行中
const TEXT_SIGNATURES = [
  "使用 CMake 构建", // round 1: pre-tool commentary
  "文件创建完成。现在用 CMake 编译", // round 2
  "编译成功，运行看看", // round 3
  "-- CMakeLists.txt", // round 4: final answer
];

// 工具模拟块
function toolSse(
  toolCallId: string,
  name: string,
  summary: string,
  result: string,
  seq: number,
): string[] {
  return [
    `id: ${seq}\ndata: {"type":"TOOL_CALL_START","toolCallId":"${toolCallId}","name":"${name}","summary":"${summary}"}\n\n`,
    `id: ${seq + 1}\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"${toolCallId}","content":"${result}","metadata":{"name":"${name}","ok":true,"summary":"${summary}"}}\n\n`,
    `id: ${seq + 2}\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n`,
  ];
}

test("多轮助手正文不应合并为一条 stored_message", async ({ page }) => {
  // ── 构造 mock SSE 事件序列 ──
  let seq = 1;
  const sseParts: string[] = [];

  // Round 1: 预工具说明 + create_dir
  sseParts.push(
    `id: ${seq++}\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n`,
  );
  sseParts.push(`id: ${seq++}\ndata: ${TEXT_SIGNATURES[0]}\n\n`);
  sseParts.push(...toolSse("tc-1", "create_dir", "create dir", "ok", seq));
  seq += 3;

  // Round 2: 创建文件后说明 + create_file
  sseParts.push(
    `id: ${seq++}\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-2","kind":"answer"}}\n\n`,
  );
  sseParts.push(
    `id: ${seq++}\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n`,
  );
  sseParts.push(`id: ${seq++}\ndata: ${TEXT_SIGNATURES[1]}\n\n`);
  sseParts.push(...toolSse("tc-2", "create_file", "create file", "ok", seq));
  seq += 3;

  // Round 3: 编译后说明 + run_command
  sseParts.push(
    `id: ${seq++}\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-3","kind":"answer"}}\n\n`,
  );
  sseParts.push(
    `id: ${seq++}\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n`,
  );
  sseParts.push(`id: ${seq++}\ndata: ${TEXT_SIGNATURES[2]}\n\n`);
  sseParts.push(...toolSse("tc-3", "run_command", "cmake --build", "ok", seq));
  seq += 3;

  // Round 4: 终答（无工具）
  sseParts.push(
    `id: ${seq++}\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-4","kind":"answer"}}\n\n`,
  );
  sseParts.push(
    `id: ${seq++}\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n`,
  );
  sseParts.push(`id: ${seq++}\ndata: ${TEXT_SIGNATURES[3]}\n\n`);

  // RUN_FINISHED
  sseParts.push(`id: ${seq++}\ndata: {"type":"RUN_FINISHED"}\n\n`);

  const sse = sseParts.join("");

  // ── 执行流 ──
  await installMockSse(page, sse);
  const sid = "s_e2e_storage_consistency_" + Date.now();
  await seedSession(page, sid);
  await sendMessage(page, "编写一个简单c++程序，使用cmake编译执行");

  // 等待流结束
  await expect(page.locator('[data-testid="status-bar"]')).toContainText(
    "就绪",
    { timeout: 25000 },
  );

  // 等待 DOM 更新完成
  await page.waitForTimeout(500);

  // ── DOM 快照：检查所有助手正文在独立气泡中 ──
  const domState = await page.evaluate(() => {
    const rows = document.querySelectorAll('[data-testid="chat-message-row"]');
    const texts: string[] = [];
    for (const row of rows) {
      texts.push(row.textContent?.trim() ?? "");
    }
    const toolCards = document.querySelectorAll(
      '[data-testid="chat-tool-card"]',
    ).length;
    return { nonToolCount: texts.length, toolCount: toolCards, texts };
  });

  console.log("=== DOM 快照（流式结束后）===");
  console.log(`非工具行数: ${domState.nonToolCount}`);
  console.log(`工具卡数: ${domState.toolCount}`);
  domState.texts.forEach((t, i) => console.log(`  行${i}: ${t.slice(0, 100)}`));

  // ── 核心断言 ──
  // Bug 特征：所有助手正文合并到一条 stored_message，
  // DOM 只有 2-3 行非工具（用户 + 合并助手 + 可能的终答碎片）
  // 期待：每轮有独立气泡，至少 2 行助手（不含用户行）

  // 注意：`installMockSse` 将所有事件一次性送达，工具事件（TOOL_CALL）与
  // turn_segment_start 同时到达时，前端的批处理使气泡轮换不充分。
  // 真实后端因 LLM/工具执行有自然延迟，事件逐步到达，行为会更好。
  // 此测试作为回归底线：确保不回退到「全部正文合并到 1 行」（即 nonToolCount ≤ 2）。
  //
  // 理想场景（真实 SSE 逐步送达）下应有更多独立气泡，由 real-llm-bubble-layout 验证。

  // v2 不可变布局：用户 + 三条已关闭 commentary + 终答。
  expect(domState.nonToolCount).toBe(5);
  expect(domState.toolCount).toBe(3);

  const assistantRowTexts = domState.texts.slice(1);
  for (const [index, commentary] of TEXT_SIGNATURES.slice(0, 3).entries()) {
    expect(assistantRowTexts[index]).toContain(commentary);
    for (const otherIndex of [0, 1, 2].filter((value) => value !== index)) {
      expect(assistantRowTexts[otherIndex]).not.toContain(commentary);
    }
  }
  const finalRow = assistantRowTexts[3] ?? "";
  expect(finalRow).toContain(TEXT_SIGNATURES[3]);
});

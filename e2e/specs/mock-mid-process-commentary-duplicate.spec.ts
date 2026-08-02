/**
 * 基于导出会话 `chat_export_20260729_210001.md`：多工具回合中
 * **每一段工具前旁白**都成对出现（中间过程重复回答）。
 *
 * 导出形态：
 *   旁白₁ → list_tree → 旁白₂（计划）×2 → create×2 → 旁白₃×2 → cmake
 *   → 旁白₄×2 → build → 旁白₅×2 → run → 终答
 *
 * 根因（写路径）：首工具后 `allow_final_answer` 曾因「已有工具行」过宽，
 * `turn_segment_end` 把中间旁白写入 `turn-final-answer`，随后 demote 再 flush
 * commentary → 本地双写；重载后服务端快照无双写（对比 `210740.md`）。
 *
 * 监控分层（本文件）：
 *   0. **流中采样**：每段旁白出现后 + 对应工具出现后再断言已见旁白 DOM 恰好 1
 *   1. 就绪后立刻：DOM / 持久化 / 导出形「## 助手」段 —— 各 needle 恰好 1（不等 hydration）
 *   2. 相邻助手正文不得完全相同（成对双写）
 *   3. 重载后再断言恰好 1（防「只靠水合变正常」）
 *
 * 手测复现（修复前）：真实 LLM 跑 C++/CMake 多工具任务 → 流式结束后立刻导出，
 * 勿先刷新页面。
 *
 * 运行：
 *   cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/mock-mid-process-commentary-duplicate.spec.ts
 */
import { expect, test } from "@playwright/test";
import {
  assertNeedlesExactlyOnceAfterReload,
  assertNeedlesExactlyOncePreHydrate,
  sampleCommentaryStepsDuringStream,
} from "../fixtures/session_assertions";
import { seedSession, sendMessage } from "../fixtures/helpers";

const STREAM_DELAY_MS = 55;
const CONV_ID = "e2e-mid-process-commentary-dup";

const INTENT_TITLE = "意图分析：执行类（直接执行）";

/** 与导出一致的各段工具前旁白（用于「恰好一条」断言）。 */
const STEP_COMMENTARIES = [
  "我先看一下工作区当前的结构，确认没有现有项目干扰。",
  "工作区是空的。我来创建一个简单的 C++ 程序，用 CMake 构建。",
  "文件已创建，现在用 CMake 配置并编译。",
  "CMake 配置成功。现在编译：",
  "编译成功，运行验证：",
] as const;

/** 流中采样：旁白出现后 / 工具可见后再查重（afterToolText 须在 transcript 中可见）。 */
const STREAM_SAMPLE_STEPS = [
  {
    commentary: STEP_COMMENTARIES[0],
    afterToolText: "list_tree",
  },
  {
    commentary: STEP_COMMENTARIES[1],
    afterToolText: "CMakeLists.txt",
  },
  {
    commentary: STEP_COMMENTARIES[2],
    afterToolText: "cmake -S . -B build",
  },
  {
    commentary: STEP_COMMENTARIES[3],
    afterToolText: "cmake --build build",
  },
  {
    commentary: STEP_COMMENTARIES[4],
    afterToolText: "./build/hello",
  },
] as const;

const FINAL_ANSWER = `已完成。项目结构如下：

\`\`\`
.
├── CMakeLists.txt
├── hello.cpp
└── build/
    └── hello
\`\`\`

Hello from CrabMate!`;

function nextId(state: { id: number }, payload: string): string {
  const line = `id: ${state.id}\ndata: ${payload}\n\n`;
  state.id += 1;
  return line;
}

function toolPair(
  state: { id: number },
  toolCallId: string,
  name: string,
  summary: string,
  result: string,
): string[] {
  return [
    nextId(
      state,
      JSON.stringify({
        type: "TOOL_CALL_START",
        toolCallId,
        name,
        summary,
      }),
    ),
    nextId(
      state,
      JSON.stringify({
        type: "TOOL_CALL_RESULT",
        toolCallId,
        content: result,
        metadata: { name, ok: true, summary },
      }),
    ),
  ];
}

/**
 * 对齐真实 LLM：每步旁白经 assistant_answer_phase + delta，
 * 再 parsing_tool_calls demote，然后工具；步与步之间 turn_segment_start(kind=answer)。
 */
function buildExportLikeSse(): string[] {
  const state = { id: 1 };
  const events: string[] = [];

  events.push(
    nextId(
      state,
      JSON.stringify({
        type: "CUSTOM",
        customType: "timeline_log",
        data: {
          kind: "intent_analysis",
          title: INTENT_TITLE,
          detail:
            "综合置信度：1.00\n主意图：execute.run_test_build\n需要澄清：false",
        },
      }),
    ),
  );

  const steps: Array<{
    commentary: string;
    tools: Array<{
      id: string;
      name: string;
      summary: string;
      result: string;
    }>;
  }> = [
    {
      commentary: STEP_COMMENTARIES[0],
      tools: [
        {
          id: "t-list",
          name: "list_tree",
          summary: "列出目录",
          result: ".",
        },
      ],
    },
    {
      commentary: STEP_COMMENTARIES[1],
      tools: [
        {
          id: "t-hello",
          name: "create_file",
          summary: "创建文件 hello.cpp",
          result: "hello.cpp",
        },
        {
          id: "t-cmake",
          name: "create_file",
          summary: "创建文件 CMakeLists.txt",
          result: "CMakeLists.txt",
        },
      ],
    },
    {
      commentary: STEP_COMMENTARIES[2],
      tools: [
        {
          id: "t-cfg",
          name: "run_command",
          summary: "cmake -S . -B build",
          result: "Configuring done",
        },
      ],
    },
    {
      commentary: STEP_COMMENTARIES[3],
      tools: [
        {
          id: "t-build",
          name: "run_command",
          summary: "cmake --build build",
          result: "Built target hello",
        },
      ],
    },
    {
      commentary: STEP_COMMENTARIES[4],
      tools: [
        {
          id: "t-run",
          name: "run_command",
          summary: "./build/hello",
          result: "Hello from CrabMate!",
        },
      ],
    },
  ];

  for (const [stepIndex, step] of steps.entries()) {
    if (stepIndex > 0) {
      events.push(
        nextId(
          state,
          JSON.stringify({
            type: "CUSTOM",
            customType: "turn_segment_start",
            data: {
              segmentId: `seg-step-${stepIndex}`,
              kind: "answer",
            },
          }),
        ),
      );
    }
    events.push(
      nextId(
        state,
        JSON.stringify({
          type: "CUSTOM",
          customType: "assistant_answer_phase",
        }),
      ),
    );
    events.push(nextId(state, step.commentary));
    events.push(
      nextId(
        state,
        JSON.stringify({
          type: "CUSTOM",
          customType: "turn_segment_end",
          data: { segmentId: `seg-commentary-${stepIndex}` },
        }),
      ),
    );
    events.push(
      nextId(
        state,
        JSON.stringify({
          type: "CUSTOM",
          customType: "parsing_tool_calls",
          data: { parsing: true },
        }),
      ),
    );
    events.push(
      nextId(
        state,
        JSON.stringify({
          type: "CUSTOM",
          customType: "tool_running",
          data: { running: true },
        }),
      ),
    );
    for (const tool of step.tools) {
      events.push(
        ...toolPair(state, tool.id, tool.name, tool.summary, tool.result),
      );
    }
    events.push(
      nextId(
        state,
        JSON.stringify({
          type: "CUSTOM",
          customType: "turn_tool_phase_end",
          data: { phase: "tool_end" },
        }),
      ),
    );
  }

  events.push(
    nextId(
      state,
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_segment_start",
        data: { segmentId: "seg-final", kind: "answer" },
      }),
    ),
  );
  events.push(
    nextId(
      state,
      JSON.stringify({
        type: "CUSTOM",
        customType: "assistant_answer_phase",
      }),
    ),
  );
  events.push(nextId(state, FINAL_ANSWER));
  events.push(
    nextId(
      state,
      JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: "1" }),
    ),
  );
  return events;
}

test("export-shaped multi-step commentaries must each appear exactly once", async ({
  page,
}) => {
  const chunks = buildExportLikeSse();
  const sid = `s_e2e_mid_process_dup_${Date.now()}`;

  await page.addInitScript(
    ({ events, delayMs, conversationId }) => {
      Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
        configurable: true,
        value: { invoke: () => Promise.resolve(null) },
      });
      const originalFetch = window.fetch.bind(window);
      window.fetch = (input, init) => {
        const url =
          typeof input === "string"
            ? input
            : input instanceof URL
              ? input.href
              : input.url;
        const method = (
          init?.method ?? (input instanceof Request ? input.method : "GET")
        ).toUpperCase();
        if (!url.includes("/chat/stream") || method !== "POST") {
          return originalFetch(input, init);
        }
        const encoder = new TextEncoder();
        const body = new ReadableStream<Uint8Array>({
          start(controller) {
            let index = 0;
            const push = () => {
              if (index >= events.length) {
                controller.close();
                return;
              }
              controller.enqueue(encoder.encode(events[index]));
              index += 1;
              window.setTimeout(push, delayMs);
            };
            push();
          },
        });
        return Promise.resolve(
          new Response(body, {
            status: 200,
            headers: {
              "content-type": "text/event-stream; charset=utf-8",
              "x-conversation-id": conversationId,
              "x-stream-job-id": "1",
            },
          }),
        );
      };
    },
    { events: chunks, delayMs: STREAM_DELAY_MS, conversationId: CONV_ID },
  );

  await seedSession(page, sid);
  await sendMessage(page, "编写一个简单c++程序，使用cmake编译执行");

  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(transcript).not.toContainText(INTENT_TITLE, { timeout: 5_000 });

  // ⓪ 流中采样（delayed SSE）：每步旁白 / 工具边界立刻查 DOM 双写
  await sampleCommentaryStepsDuringStream({
    page,
    sid,
    steps: STREAM_SAMPLE_STEPS,
    timeoutMs: 45_000,
  });

  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 60_000,
  });
  await expect(transcript).toContainText("Hello from CrabMate", {
    timeout: 15_000,
  });

  // ① 就绪瞬间（不等 server_revision / hydration 稳定）
  await assertNeedlesExactlyOncePreHydrate({
    page,
    sid,
    needles: STEP_COMMENTARIES,
    label: "ready-immediate",
  });

  // ② 重载后仍恰好一条（禁止只靠水合「变正常」）
  await assertNeedlesExactlyOnceAfterReload({
    page,
    sid,
    needles: STEP_COMMENTARIES,
  });
});

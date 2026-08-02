/**
 * 基于导出会话 `chat_export_20260727_214636.md` 的高保真 mock SSE：
 *   意图分析（qa.codebase / 需澄清）→ 开场白 → 6× 并行 read_file → 长终答（分块流式）
 * 并监控 **助手消息气泡**（section.chat-tui-turn--assistant）不得在旁白出现后归零。
 *
 * 运行：
 *   cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/mock-export-analyze-project-flicker.spec.ts
 */
import { expect, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const STREAM_DELAY_MS = 90;
const CONV_ID = "e2e-export-analyze-project";

const INTENT_TITLE = "意图分析：问答类（直接回复）";
const COMMENTARY =
  "好的，这是一个 C++ 项目，结构简洁。让我先快速浏览关键文件，了解项目全貌。";
// TUI 行级 Markdown 会吃掉标题行 / fence / 部分粗体前后缀；断言用最终 DOM 仍保留的纯文本。
const FINAL_MARKERS = [
  "项目已全部读入。下面是分析：",
  "简洁的 C++ 演示项目",
  "值得注意的差异",
  "调整？",
] as const;

const READ_FILES = [
  "README.md",
  "CMakeLists.txt",
  "hello.cpp",
  "hello_lib.h",
  "hello_lib.cpp",
  "test_hello.cpp",
] as const;

function chunkText(text: string, size: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < text.length; i += size) {
    out.push(text.slice(i, i + size));
  }
  return out.length ? out : [text];
}

const FINAL_ANSWER = `项目已全部读入。下面是分析：

---

## 项目概览：Hello CrabMate

这是一个**简洁的 C++ 演示项目**，使用 CMake 构建，展示了：

### 结构

\`\`\`
├── CMakeLists.txt
├── hello.cpp
├── hello_lib.h
├── hello_lib.cpp
└── test_hello.cpp
\`\`\`

### 一个值得注意的差异

README 需要同步更新才能准确反映当前接口。

### 构建与运行

\`\`\`bash
cmake -S . -B build
cmake --build build
\`\`\`

---

需要我做什么？比如构建验证、补充 README 接口说明、添加新功能，或做其他调整？`;

function buildExportLikeSse(): string[] {
  let id = 1;
  const next = (payload: string) => {
    const line = `id: ${id}\ndata: ${payload}\n\n`;
    id += 1;
    return line;
  };

  const events: string[] = [];
  events.push(
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "timeline_log",
        data: {
          kind: "intent_analysis",
          title: INTENT_TITLE,
          detail:
            "综合置信度：0.60\n主意图：qa.codebase\n需要澄清：true\n决策来源：L2（置信度 0.60）",
        },
      }),
    ),
  );
  events.push(
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "assistant_answer_phase",
      }),
    ),
  );
  events.push(next(COMMENTARY));
  events.push(
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_segment_end",
        data: { segmentId: "seg-commentary" },
      }),
    ),
  );
  // 单独一帧 demote：与真实 LLM 的 parsing_tool_calls 对齐，跨 Effect 绘制。
  events.push(
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "parsing_tool_calls",
        data: { parsing: true },
      }),
    ),
  );
  events.push(
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "tool_running",
        data: { running: true },
      }),
    ),
  );

  for (const [i, file] of READ_FILES.entries()) {
    const toolCallId = `t-read-${i + 1}`;
    events.push(
      next(
        JSON.stringify({
          type: "TOOL_CALL_START",
          toolCallId,
          name: "read_file",
          summary: `读取文件 ${file}`,
        }),
      ),
    );
  }
  for (const [i, file] of READ_FILES.entries()) {
    const toolCallId = `t-read-${i + 1}`;
    events.push(
      next(
        JSON.stringify({
          type: "TOOL_CALL_RESULT",
          toolCallId,
          content: `ok: ${file}`,
          metadata: {
            name: "read_file",
            ok: true,
            summary: `read file: ${file}`,
          },
        }),
      ),
    );
  }
  events.push(
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_tool_phase_end",
        data: { phase: "tool_end" },
      }),
    ),
  );
  events.push(
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_segment_start",
        data: { segmentId: "seg-final", kind: "answer" },
      }),
    ),
  );
  events.push(
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "assistant_answer_phase",
      }),
    ),
  );
  for (const part of chunkText(FINAL_ANSWER, 48)) {
    events.push(next(part));
  }
  events.push(
    next(JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: "1" })),
  );
  events.push(
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "conversation_saved",
        data: { revision: 1 },
      }),
    ),
  );
  return events;
}

type AssistantGap = {
  reason: string;
  tMs: number;
  chunkIndex: number;
  assistantCount: number;
  commentaryVisible: boolean;
};

test("export-shaped stream: assistant bubble must not vanish after commentary", async ({
  page,
}) => {
  const sseChunks = buildExportLikeSse();

  await page.addInitScript(
    ({ chunks, delayMs, convId, commentary }) => {
      Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
        configurable: true,
        value: { invoke: () => Promise.resolve(null) },
      });

      const state = globalThis as typeof globalThis & {
        __cmChunkIndex?: number;
        __cmStartedAt?: number;
        __cmAssistantGaps?: AssistantGap[];
        __cmSamples?: number;
        __cmStopAssistantFlicker?: () => {
          gaps: AssistantGap[];
          samples: number;
          lastChunk: number;
        };
      };
      state.__cmChunkIndex = -1;
      state.__cmAssistantGaps = [];
      state.__cmSamples = 0;

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
        state.__cmStartedAt = performance.now();
        const encoder = new TextEncoder();
        const body = new ReadableStream<Uint8Array>({
          start(controller) {
            let index = 0;
            const push = () => {
              if (index >= chunks.length) {
                controller.close();
                return;
              }
              state.__cmChunkIndex = index;
              controller.enqueue(encoder.encode(chunks[index]));
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
              "x-conversation-id": convId,
              "x-stream-job-id": "1",
            },
          }),
        );
      };

      (
        globalThis as typeof globalThis & {
          __cmInstallAssistantBubbleMonitor?: () => void;
        }
      ).__cmInstallAssistantBubbleMonitor = () => {
        const gaps: AssistantGap[] = [];
        let commentarySeen = false;
        const sample = () => {
          state.__cmSamples = (state.__cmSamples ?? 0) + 1;
          const tMs = Math.round(
            performance.now() - (state.__cmStartedAt ?? performance.now()),
          );
          const assistants = [
            ...document.querySelectorAll<HTMLElement>(
              "section.chat-tui-turn--assistant",
            ),
          ];
          // 只关心正文助手气泡：含旁白片段，或非空非意图卡
          const bodyAssistants = assistants.filter((el) => {
            const text = (el.innerText ?? "").replace(/\s+/g, " ");
            if (!text.trim()) return false;
            if (text.includes("意图分析")) return false;
            return true;
          });
          const commentaryVisible = bodyAssistants.some((el) =>
            (el.innerText ?? "").includes(commentary.slice(0, 12)),
          );
          if (commentaryVisible) commentarySeen = true;
          if (commentarySeen && bodyAssistants.length === 0) {
            gaps.push({
              reason: "assistant_body_sections_zero",
              tMs,
              chunkIndex: state.__cmChunkIndex ?? -1,
              assistantCount: 0,
              commentaryVisible: false,
            });
          }
          if (commentarySeen && !commentaryVisible) {
            gaps.push({
              reason: "commentary_missing_from_assistant_bubbles",
              tMs,
              chunkIndex: state.__cmChunkIndex ?? -1,
              assistantCount: bodyAssistants.length,
              commentaryVisible: false,
            });
          }
        };
        const root = document.querySelector(
          '[data-testid="chat-tui-transcript"]',
        );
        if (!root) throw new Error("transcript missing");
        const mo = new MutationObserver(sample);
        mo.observe(root, {
          childList: true,
          characterData: true,
          subtree: true,
        });
        const timer = window.setInterval(sample, 8);
        state.__cmStopAssistantFlicker = () => {
          mo.disconnect();
          window.clearInterval(timer);
          state.__cmAssistantGaps = gaps;
          return {
            gaps,
            samples: state.__cmSamples ?? 0,
            lastChunk: state.__cmChunkIndex ?? -1,
          };
        };
      };
    },
    {
      chunks: sseChunks,
      delayMs: STREAM_DELAY_MS,
      convId: CONV_ID,
      commentary: COMMENTARY,
    },
  );

  await seedSession(page, `s_e2e_export_analyze_${Date.now()}`);
  await expect(page.getByTestId("chat-tui-stream-view")).toBeVisible();
  await page.evaluate(() => {
    (
      globalThis as typeof globalThis & {
        __cmInstallAssistantBubbleMonitor?: () => void;
      }
    ).__cmInstallAssistantBubbleMonitor?.();
  });

  await sendMessage(page, "分析当前项目");

  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(transcript).not.toContainText(INTENT_TITLE, { timeout: 5_000 });
  await expect(transcript).toContainText(COMMENTARY, { timeout: 20_000 });
  await expect(transcript).toContainText("README.md", { timeout: 20_000 });
  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 45_000,
  });
  for (const marker of FINAL_MARKERS) {
    await expect(transcript).toContainText(marker, { timeout: 10_000 });
  }
  await page.waitForTimeout(600);

  const result = await page.evaluate(() => {
    const state = globalThis as typeof globalThis & {
      __cmStopAssistantFlicker?: () => {
        gaps: AssistantGap[];
        samples: number;
        lastChunk: number;
      };
    };
    return (
      state.__cmStopAssistantFlicker?.() ?? {
        gaps: [],
        samples: 0,
        lastChunk: -1,
      }
    );
  });

  expect(
    result.gaps,
    `assistant bubble vanished: ${JSON.stringify(result.gaps)}; samples=${result.samples}; lastChunk=${result.lastChunk}`,
  ).toEqual([]);
  expect(result.samples).toBeGreaterThan(20);
});

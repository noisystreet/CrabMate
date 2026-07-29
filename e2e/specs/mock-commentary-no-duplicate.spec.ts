/**
 * 工具前旁白写路径：恰好一条，且不得在多轮时被掏空。
 *
 * 失败模式：
 *   - 双写：demote keep-ui + commentary 投影 + finalize 升格 → 同文两条
 *   - 零写：移交条件误用「会话里任意 commentary 已存在」→ 多轮时清掉本轮尚未投影的 loading
 *
 * 运行：
 *   cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/mock-commentary-no-duplicate.spec.ts
 */
import { expect, Page, Route, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const STREAM_DELAY_MS = 80;
const COMMENTARY =
  "在继续完善前，我先看看当前有哪些已知的待办事项和已有文档结构。";
const FINAL_ANSWER = "已查看待办与文档结构，下一步可以继续完善。";
const PRIOR_COMMENTARY = "上一轮已读过 README，先记下结构。";
const PRIOR_ANSWER = "上一轮分析完成。";
const SECOND_COMMENTARY =
  "在继续完善前，我先看看当前有哪些已知的待办事项和已有文档结构。";
const SECOND_ANSWER = "第二轮已查看待办。";

type PersistedMessage = {
  id: string;
  role: string;
  text?: string;
  is_tool?: boolean;
  state?: string | null;
};

type PersistedSession = {
  id: string;
  messages?: PersistedMessage[];
};

function toolTurnSse(
  convSuffix: string,
  commentary: string,
  answer: string,
  toolCallId: string,
): string[] {
  let id = 1;
  const next = (payload: string) => {
    const line = `id: ${id}\ndata: ${payload}\n\n`;
    id += 1;
    return line;
  };
  return [
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "assistant_answer_phase",
      }),
    ),
    next(commentary),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_segment_end",
        data: { segmentId: `seg-commentary-${convSuffix}` },
      }),
    ),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "parsing_tool_calls",
        data: { parsing: true },
      }),
    ),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "tool_running",
        data: { running: true },
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_START",
        toolCallId,
        name: "read_file",
        summary: `读取文件 ${toolCallId}`,
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_RESULT",
        toolCallId,
        content: "ok",
        metadata: {
          name: "read_file",
          ok: true,
          summary: `read file: ${toolCallId}`,
        },
      }),
    ),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_tool_phase_end",
        data: { phase: "tool_end" },
      }),
    ),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_segment_start",
        data: { segmentId: `seg-final-${convSuffix}`, kind: "answer" },
      }),
    ),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "assistant_answer_phase",
      }),
    ),
    next(answer),
    next(
      JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: convSuffix }),
    ),
  ];
}

async function installDelayedStream(
  page: Page,
  chunks: string[],
  convId: string,
) {
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
    { events: chunks, delayMs: STREAM_DELAY_MS, conversationId: convId },
  );
}

async function installSequentialDelayedStreams(
  page: Page,
  streams: string[][],
) {
  let callIndex = 0;
  await page.route("**/chat/stream", async (route: Route) => {
    if (route.request().method() !== "POST") {
      return route.continue();
    }
    const chunks = streams[Math.min(callIndex, streams.length - 1)] ?? [];
    callIndex += 1;
    const body = chunks.join("");
    // 用分块 ReadableStream 模拟时序；route.fulfill 不支持自定义 stream，改为整包 + 短延迟由页面侧处理。
    // 此处改走 fulfill 整包：多轮边界仍覆盖「旧 commentary 已存在」的移交条件。
    return route.fulfill({
      status: 200,
      headers: {
        "content-type": "text/event-stream; charset=utf-8",
        "x-conversation-id": "e2e-commentary-multi",
        "x-stream-job-id": String(callIndex),
      },
      body,
    });
  });
}

async function persistedSession(page: Page, sid: string) {
  return page.evaluate(async (sessionId) => {
    const response = await fetch("/user-data/workspaces/current/sessions");
    const data = await response.json();
    return (
      (data.sessions as PersistedSession[] | undefined)?.find(
        (session) => session.id === sessionId,
      ) ?? null
    );
  }, sid);
}

function assistantSectionsWithText(page: Page, text: string) {
  return page.evaluate((needle) => {
    const sections = [
      ...document.querySelectorAll<HTMLElement>(
        "section.chat-tui-turn--assistant",
      ),
    ];
    return sections
      .map((el) => (el.innerText ?? "").replace(/\s+/g, " ").trim())
      .filter((t) => t.includes(needle) && !t.includes("意图分析"));
  }, text);
}

test("pre-tool commentary must persist exactly once after ready", async ({
  page,
}) => {
  const sseChunks = toolTurnSse(
    "1",
    COMMENTARY,
    FINAL_ANSWER,
    "t-read-todolist",
  );
  const sid = `s_e2e_commentary_dedupe_${Date.now()}`;
  await installDelayedStream(page, sseChunks, "e2e-commentary-no-duplicate");
  await seedSession(page, sid);
  await sendMessage(page, "继续完善前先看待办");

  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 45_000,
  });
  await expect(transcript).toContainText(FINAL_ANSWER, { timeout: 15_000 });
  // 流结束后旁白仍须可见（防「双写修成零写」）。
  await expect(transcript).toContainText(COMMENTARY, { timeout: 5_000 });

  expect(await assistantSectionsWithText(page, COMMENTARY)).toHaveLength(1);

  await expect
    .poll(
      async () => {
        const session = await persistedSession(page, sid);
        return (session?.messages ?? [])
          .filter(
            (message) =>
              message.role === "assistant" &&
              !message.is_tool &&
              (message.text ?? "").trim() === COMMENTARY,
          )
          .map((message) => message.id);
      },
      { timeout: 10_000 },
    )
    .toEqual([expect.stringMatching(/^turn-commentary-/)]);
});

test("second-turn commentary must not vanish when prior commentary exists", async ({
  page,
}) => {
  const sid = `s_e2e_commentary_multiturn_${Date.now()}`;
  await installSequentialDelayedStreams(page, [
    toolTurnSse("1", PRIOR_COMMENTARY, PRIOR_ANSWER, "t-read-prior"),
    toolTurnSse("2", SECOND_COMMENTARY, SECOND_ANSWER, "t-read-second"),
  ]);
  await seedSession(page, sid);

  await sendMessage(page, "先看 README");
  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 45_000,
  });
  await expect(page.getByTestId("chat-tui-transcript")).toContainText(
    PRIOR_COMMENTARY,
  );
  await expect(page.getByTestId("chat-tui-transcript")).toContainText(
    PRIOR_ANSWER,
  );

  await sendMessage(page, "继续完善前先看待办");
  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 45_000,
  });
  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(transcript).toContainText(SECOND_ANSWER, { timeout: 15_000 });
  // 关键：上轮旁注仍在的前提下，本轮旁白不得被错误移交掏空。
  await expect(transcript).toContainText(SECOND_COMMENTARY, { timeout: 5_000 });
  await expect(transcript).toContainText(PRIOR_COMMENTARY);

  expect(await assistantSectionsWithText(page, SECOND_COMMENTARY)).toHaveLength(
    1,
  );
  expect(await assistantSectionsWithText(page, PRIOR_COMMENTARY)).toHaveLength(
    1,
  );

  await expect
    .poll(
      async () => {
        const session = await persistedSession(page, sid);
        const texts = (session?.messages ?? [])
          .filter(
            (message) =>
              message.role === "assistant" &&
              !message.is_tool &&
              ((message.text ?? "").trim() === PRIOR_COMMENTARY ||
                (message.text ?? "").trim() === SECOND_COMMENTARY),
          )
          .map((message) => (message.text ?? "").trim());
        return texts.sort();
      },
      { timeout: 10_000 },
    )
    .toEqual([PRIOR_COMMENTARY, SECOND_COMMENTARY].sort());
});

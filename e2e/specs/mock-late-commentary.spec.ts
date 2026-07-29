/**
 * 晚到旁注（形态 A）：`tool_call` / 工具占位先于旁白 plain delta。
 *
 * 对齐金样 `late_commentary_delta_after_tool_call`：
 *   read 工具 → segment_start(before create) → create 工具 → **晚到**旁白 delta → segment_end
 * 期望：旁白仍挂在 create 前，DOM/持久化恰好一条，且位于 create 工具行之前。
 *
 * 运行：
 *   cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/mock-late-commentary.spec.ts
 */
import { expect, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const STREAM_DELAY_MS = 70;
const CONV_ID = "e2e-late-commentary";
const LATE_COMMENTARY = "工作区是空的。";
const FINAL_ANSWER = "已创建文件，工作区不再为空。";
const CREATE_TOOL_ID = "tc_create";
const READ_TOOL_ID = "tc_read";

type PersistedMessage = {
  id: string;
  role: string;
  text?: string;
  is_tool?: boolean;
  tool_call_id?: string | null;
};

type PersistedSession = {
  id: string;
  messages?: PersistedMessage[];
};

function buildLateCommentarySse(): string[] {
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
    // 先工具：无旁白
    next(
      JSON.stringify({
        type: "TOOL_CALL_START",
        toolCallId: READ_TOOL_ID,
        name: "read_dir",
        summary: "列出目录",
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_RESULT",
        toolCallId: READ_TOOL_ID,
        content: "empty",
        metadata: {
          name: "read_dir",
          ok: true,
          summary: "列出目录",
        },
      }),
    ),
    // 为 create 开旁注段（正文尚未到达）
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_segment_start",
        data: {
          segmentId: "seg-before-tc_create",
          kind: "commentary",
          beforeToolCallId: CREATE_TOOL_ID,
        },
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_START",
        toolCallId: CREATE_TOOL_ID,
        name: "create_file",
        summary: "创建文件",
      }),
    ),
    // 晚到旁白：在 create 工具声明之后才到
    next(LATE_COMMENTARY),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_segment_end",
        data: { segmentId: "seg-before-tc_create" },
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_RESULT",
        toolCallId: CREATE_TOOL_ID,
        content: "ok",
        metadata: {
          name: "create_file",
          ok: true,
          summary: "创建文件",
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
        data: { segmentId: "seg-final", kind: "answer" },
      }),
    ),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "assistant_answer_phase",
      }),
    ),
    next(FINAL_ANSWER),
    next(JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: "1" })),
  ];
}

async function persistedSession(
  page: import("@playwright/test").Page,
  sid: string,
) {
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

test("late commentary after tool_call still sits before its tool exactly once", async ({
  page,
}) => {
  const chunks = buildLateCommentarySse();
  const sid = `s_e2e_late_commentary_${Date.now()}`;

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
  await sendMessage(page, "在空工作区创建文件");

  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 45_000,
  });
  await expect(transcript).toContainText(FINAL_ANSWER, { timeout: 15_000 });
  // 流结束后旁白仍须可见（防晚到 delta 丢失或落到工具后却被读侧重排丢掉）
  await expect(transcript).toContainText(LATE_COMMENTARY, { timeout: 5_000 });

  const assistantHits = await page.evaluate((commentary) => {
    return [
      ...document.querySelectorAll<HTMLElement>(
        "section.chat-tui-turn--assistant",
      ),
    ]
      .map((el) => (el.innerText ?? "").replace(/\s+/g, " ").trim())
      .filter(
        (text) => text.includes(commentary) && !text.includes("意图分析"),
      );
  }, LATE_COMMENTARY);
  expect(
    assistantHits,
    `late commentary must appear exactly once in DOM: ${JSON.stringify(assistantHits)}`,
  ).toHaveLength(1);

  // DOM 顺序：旁白 section 须出现在「创建文件」工具 turn 之前
  const domOrder = await page.evaluate((commentary) => {
    const turns = [
      ...document.querySelectorAll<HTMLElement>("section.chat-tui-turn"),
    ];
    const commentaryIdx = turns.findIndex((el) =>
      (el.innerText ?? "").includes(commentary),
    );
    const createToolIdx = turns.findIndex((el) => {
      const text = el.innerText ?? "";
      return (
        el.classList.contains("chat-tui-turn--tool") &&
        (text.includes("创建文件") || text.includes("create_file"))
      );
    });
    return { commentaryIdx, createToolIdx };
  }, LATE_COMMENTARY);
  expect(domOrder.commentaryIdx).toBeGreaterThanOrEqual(0);
  expect(domOrder.createToolIdx).toBeGreaterThanOrEqual(0);
  expect(
    domOrder.commentaryIdx,
    `commentary must precede create tool in DOM: ${JSON.stringify(domOrder)}`,
  ).toBeLessThan(domOrder.createToolIdx);

  await expect
    .poll(
      async () => {
        const session = await persistedSession(page, sid);
        const messages = session?.messages ?? [];
        const commentaryRows = messages.filter(
          (message) =>
            message.role === "assistant" &&
            !message.is_tool &&
            (message.text ?? "").trim() === LATE_COMMENTARY,
        );
        const createToolIdx = messages.findIndex(
          (message) =>
            message.is_tool && message.tool_call_id === CREATE_TOOL_ID,
        );
        const commentaryIdx = messages.findIndex(
          (message) =>
            message.id === `turn-commentary-${CREATE_TOOL_ID}` ||
            ((message.text ?? "").trim() === LATE_COMMENTARY &&
              !message.is_tool),
        );
        return {
          commentaryIds: commentaryRows.map((row) => row.id),
          commentaryIdx,
          createToolIdx,
        };
      },
      { timeout: 10_000 },
    )
    .toEqual({
      commentaryIds: [`turn-commentary-${CREATE_TOOL_ID}`],
      commentaryIdx: expect.any(Number),
      createToolIdx: expect.any(Number),
    });

  const session = await persistedSession(page, sid);
  const messages = session?.messages ?? [];
  const commentaryIdx = messages.findIndex(
    (message) => message.id === `turn-commentary-${CREATE_TOOL_ID}`,
  );
  const createToolIdx = messages.findIndex(
    (message) => message.is_tool && message.tool_call_id === CREATE_TOOL_ID,
  );
  expect(commentaryIdx).toBeGreaterThanOrEqual(0);
  expect(createToolIdx).toBeGreaterThanOrEqual(0);
  expect(
    commentaryIdx,
    "persisted commentary must sit before its anchored create tool",
  ).toBeLessThan(createToolIdx);
});

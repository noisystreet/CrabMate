/**
 * 工具前旁白不得双写：demote keep-ui → commentary 投影 → finalize loading
 * 不得再留下第二条同文普通 assistant（导出/持久化可见）。
 *
 * 复现路径对齐真实 LLM：plain delta → parsing_tool_calls demote → tool_call →
 * tool_result finalize（旧 loading 仍握旁白正文时会升格为第二条助手行）。
 *
 * 运行：
 *   cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/mock-commentary-no-duplicate.spec.ts
 */
import { expect, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const STREAM_DELAY_MS = 80;
const CONV_ID = "e2e-commentary-no-duplicate";
const COMMENTARY =
  "在继续完善前，我先看看当前有哪些已知的待办事项和已有文档结构。";
const FINAL_ANSWER = "已查看待办与文档结构，下一步可以继续完善。";

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

function buildSse(): string[] {
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
  // 单独一帧 demote：与真实 LLM 的 parsing_tool_calls 对齐。
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
  events.push(
    next(
      JSON.stringify({
        type: "TOOL_CALL_START",
        toolCallId: "t-read-todolist",
        name: "read_file",
        summary: "读取文件 docs/待办清单.md",
      }),
    ),
  );
  events.push(
    next(
      JSON.stringify({
        type: "TOOL_CALL_RESULT",
        toolCallId: "t-read-todolist",
        content: "ok",
        metadata: {
          name: "read_file",
          ok: true,
          summary: "read file: docs/待办清单.md",
        },
      }),
    ),
  );
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
  events.push(next(FINAL_ANSWER));
  events.push(
    next(JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: "1" })),
  );
  return events;
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

test("pre-tool commentary must not persist as duplicate assistant rows", async ({
  page,
}) => {
  const sseChunks = buildSse();
  const sid = `s_e2e_commentary_dedupe_${Date.now()}`;

  await page.addInitScript(
    ({ chunks, delayMs, convId }) => {
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
              if (index >= chunks.length) {
                controller.close();
                return;
              }
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
    },
    { chunks: sseChunks, delayMs: STREAM_DELAY_MS, convId: CONV_ID },
  );

  await seedSession(page, sid);
  await sendMessage(page, "继续完善前先看待办");

  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(transcript).toContainText(COMMENTARY, { timeout: 20_000 });
  await expect(transcript).toContainText("待办清单", { timeout: 20_000 });
  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 45_000,
  });
  await expect(transcript).toContainText(FINAL_ANSWER, { timeout: 15_000 });

  // DOM（TUI）：旁白正文只应出现在一个助手 section（不含意图卡）。
  const domCommentaryHits = await page.evaluate((commentary) => {
    const sections = [
      ...document.querySelectorAll<HTMLElement>(
        "section.chat-tui-turn--assistant",
      ),
    ];
    return sections
      .map((el) => (el.innerText ?? "").replace(/\s+/g, " ").trim())
      .filter(
        (text) => text.includes(commentary) && !text.includes("意图分析"),
      );
  }, COMMENTARY);
  expect(
    domCommentaryHits,
    `DOM duplicate commentary bubbles: ${JSON.stringify(domCommentaryHits)}`,
  ).toHaveLength(1);

  // 持久化：同文助手行只能一条（commentary 投影行）；不得再有升格后的 loading 副本。
  await expect
    .poll(
      async () => {
        const session = await persistedSession(page, sid);
        const assistants = (session?.messages ?? []).filter(
          (message) =>
            message.role === "assistant" &&
            !message.is_tool &&
            (message.text ?? "").trim() === COMMENTARY,
        );
        return assistants.map((message) => ({
          id: message.id,
          state: message.state ?? null,
        }));
      },
      { timeout: 10_000 },
    )
    .toEqual([
      expect.objectContaining({
        id: expect.stringMatching(/^turn-commentary-/),
      }),
    ]);
});

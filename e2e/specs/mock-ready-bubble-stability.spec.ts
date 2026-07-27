/**
 * TUI 主列：已定稿 assistant 段在后续工具/流式过程中不得消失或改文。
 * （旧版依赖 chat-message-row；默认主列已是 chat-tui-transcript。）
 */
import { expect, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const STREAM_CHUNK_DELAY_MS = 350;
const FIRST_READY_TEXT = "第一段已经完成。";
const SECOND_READY_TEXT = "第二段也已完成。";
const FINAL_TEXT = "这是最终回答。";

const SSE_CHUNKS = [
  'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
  `id: 2\ndata: ${FIRST_READY_TEXT}\n\n`,
  'id: 3\ndata: {"type":"TOOL_CALL_START","toolCallId":"t1","name":"list_tree","summary":"列出目录"}\n\n',
  'id: 4\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"t1","content":"empty","metadata":{"name":"list_tree","ok":true,"summary":"列出目录"}}\n\n',
  'id: 5\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
  'id: 6\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-2","kind":"answer"}}\n\n',
  'id: 7\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
  `id: 8\ndata: ${SECOND_READY_TEXT}\n\n`,
  'id: 9\ndata: {"type":"TOOL_CALL_START","toolCallId":"t2","name":"read_file","summary":"读取文件"}\n\n',
  'id: 10\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"t2","content":"ok","metadata":{"name":"read_file","ok":true,"summary":"读取文件"}}\n\n',
  'id: 11\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
  'id: 12\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-3","kind":"answer"}}\n\n',
  'id: 13\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
  `id: 14\ndata: ${FINAL_TEXT}\n\n`,
  'id: 15\ndata: {"type":"RUN_FINISHED","threadId":"","runId":"1"}\n\n',
];

type ReadyBubbleViolation = {
  id: string;
  expectedText: string;
  actualText: string | null;
  kind: "changed" | "disappeared";
  chunkIndex: number;
};

type ReadyBubbleMonitor = {
  freeze: (textSignature: string) => void;
  violations: ReadyBubbleViolation[];
  frozen: Array<{ id: string; text: string }>;
  observer: MutationObserver;
};

test("ready TUI turns remain immutable and visible while streaming", async ({
  page,
}) => {
  await page.addInitScript(
    ({ chunks, delayMs }) => {
      Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
        configurable: true,
        value: { invoke: () => Promise.resolve(null) },
      });

      const state = globalThis as typeof globalThis & {
        __cmChunkIndex?: number;
      };
      state.__cmChunkIndex = -1;

      const originalFetch = window.fetch.bind(window);
      window.fetch = (input, init) => {
        const url =
          typeof input === "string"
            ? input
            : input instanceof URL
              ? input.href
              : input.url;
        const method =
          init?.method ?? (input instanceof Request ? input.method : "GET");
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
              "x-conversation-id": "e2e-ready-bubble-stability",
              "x-stream-job-id": "1",
            },
          }),
        );
      };
    },
    { chunks: SSE_CHUNKS, delayMs: STREAM_CHUNK_DELAY_MS },
  );

  await seedSession(page, `s_e2e_ready_bubble_${Date.now()}`);
  await expect(page.getByTestId("chat-tui-stream-view")).toBeVisible();
  await sendMessage(page, "检查已完成气泡在流式生成期间保持稳定");

  const freezeReadyTurn = async (
    textSignature: string,
    minimumChunk: number,
  ) => {
    await page.waitForFunction(
      ({ signature, chunkIndex }) => {
        const state = globalThis as typeof globalThis & {
          __cmChunkIndex?: number;
        };
        if ((state.__cmChunkIndex ?? -1) < chunkIndex) return false;
        return [
          ...document.querySelectorAll<HTMLElement>(
            "section.chat-tui-turn[data-tui-msg-id]",
          ),
        ].some(
          (row) =>
            (row.innerText ?? "").includes(signature) &&
            row.getAttribute("data-tui-live") !== "1" &&
            !row.classList.contains("chat-tui-turn--loading"),
        );
      },
      { signature: textSignature, chunkIndex: minimumChunk },
      { timeout: 25_000 },
    );

    await page.evaluate((signature) => {
      const normalize = (text: string | null) =>
        (text ?? "").replace(/\s+/g, " ").trim();
      const state = globalThis as typeof globalThis & {
        __cmReadyBubbleMonitor?: ReadyBubbleMonitor;
        __cmChunkIndex?: number;
      };

      if (!state.__cmReadyBubbleMonitor) {
        const frozen = new Map<string, string>();
        const violations: ReadyBubbleViolation[] = [];
        const recordViolation = (violation: ReadyBubbleViolation) => {
          if (
            violations.some(
              (existing) =>
                existing.id === violation.id &&
                existing.kind === violation.kind &&
                existing.actualText === violation.actualText &&
                existing.chunkIndex === violation.chunkIndex,
            )
          ) {
            return;
          }
          violations.push(violation);
        };
        const inspect = () => {
          for (const [id, expectedText] of frozen) {
            const row = document.querySelector<HTMLElement>(
              `section.chat-tui-turn[data-tui-msg-id="${id.replace(/"/g, '\\"')}"]`,
            );
            if (!row) {
              recordViolation({
                id,
                expectedText,
                actualText: null,
                kind: "disappeared",
                chunkIndex: state.__cmChunkIndex ?? -1,
              });
              continue;
            }
            const actualText = normalize(row.innerText);
            if (!actualText.includes(expectedText)) {
              recordViolation({
                id,
                expectedText,
                actualText,
                kind: "disappeared",
                chunkIndex: state.__cmChunkIndex ?? -1,
              });
              continue;
            }
            if (
              actualText !== expectedText &&
              !actualText.includes(expectedText)
            ) {
              recordViolation({
                id,
                expectedText,
                actualText,
                kind: "changed",
                chunkIndex: state.__cmChunkIndex ?? -1,
              });
            }
          }
        };
        let inspectionScheduled = false;
        const inspectAfterPaint = () => {
          if (inspectionScheduled) return;
          inspectionScheduled = true;
          requestAnimationFrame(() => {
            window.setTimeout(() => {
              inspectionScheduled = false;
              inspect();
            }, 0);
          });
        };
        const root = document.querySelector(
          '[data-testid="chat-tui-transcript"]',
        );
        if (!root) throw new Error("tui transcript root not found");
        const observer = new MutationObserver(inspectAfterPaint);
        observer.observe(root, {
          childList: true,
          characterData: true,
          subtree: true,
        });
        state.__cmReadyBubbleMonitor = {
          freeze(text) {
            const row = [
              ...document.querySelectorAll<HTMLElement>(
                "section.chat-tui-turn[data-tui-msg-id]",
              ),
            ].find(
              (candidate) =>
                (candidate.innerText ?? "").includes(text) &&
                candidate.getAttribute("data-tui-live") !== "1" &&
                !candidate.classList.contains("chat-tui-turn--loading"),
            );
            const id = row?.getAttribute("data-tui-msg-id");
            if (!row || !id) throw new Error(`ready turn not found: ${text}`);
            if (!frozen.has(id)) {
              frozen.set(id, text);
              this.frozen.push({
                id,
                text,
              });
            }
          },
          violations,
          frozen: [],
          observer,
        };
      }

      state.__cmReadyBubbleMonitor.freeze(signature);
    }, textSignature);
  };

  await freezeReadyTurn(FIRST_READY_TEXT, 4);
  await freezeReadyTurn(SECOND_READY_TEXT, 10);

  await expect(page.getByTestId("chat-tui-transcript")).toContainText(
    FINAL_TEXT,
    { timeout: 20_000 },
  );
  await expect(page.locator('[data-testid="status-bar"]')).toContainText(
    "就绪",
  );
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => requestAnimationFrame(() => resolve())),
  );

  const result = await page.evaluate(() => {
    const state = globalThis as typeof globalThis & {
      __cmReadyBubbleMonitor?: ReadyBubbleMonitor;
    };
    const monitor = state.__cmReadyBubbleMonitor;
    if (!monitor) throw new Error("ready bubble monitor was not installed");
    monitor.observer.disconnect();
    return {
      frozen: monitor.frozen,
      violations: monitor.violations,
      visibleIds: [
        ...document.querySelectorAll<HTMLElement>(
          "section.chat-tui-turn[data-tui-msg-id]",
        ),
      ].map((row) => row.getAttribute("data-tui-msg-id") ?? ""),
    };
  });

  expect(
    result.violations,
    `ready turn violated: ${JSON.stringify(result.violations)}`,
  ).toEqual([]);
  expect(result.frozen).toHaveLength(2);
  for (const bubble of result.frozen) {
    expect(result.visibleIds).toContain(bubble.id);
  }
});

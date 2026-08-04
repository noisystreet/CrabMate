/**
 * 生成过程中：工具前描述（旁白）应先出现，再展示工具调用。
 *
 * 真实 SSE 顺序（delayed）：
 *   assistant_answer_phase → 旁白 delta → parsing_tool_calls → TOOL_CALL_START → …
 *
 * 期望：
 *   1. 旁白先于工具出现在 transcript
 *   2. 自工具首次可见起，任意 DOM 快照中旁白 turn 下标均 < 工具 turn
 *
 * 失败形态（用户反馈「现在是反的」）：
 *   工具行先插入且 pin loading 到末尾 → 旁白仍挂 loading、落在工具之后；
 *   或 flush 前若干帧可见「工具在上、描述在下」。
 *
 * 运行：
 *   cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/mock-commentary-before-tool-order.spec.ts
 */
import { expect, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const STREAM_DELAY_MS = 120;
const CONV_ID = "e2e-commentary-before-tool-order";
const COMMENTARY = "我先看一下工作区当前的结构，确认没有现有项目干扰。";
const TOOL_SUMMARY = "列出目录";
const TOOL_NAME = "list_tree";
const TOOL_CALL_ID = "tc_list";
const FINAL_ANSWER = "工作区为空，可以开始创建项目。";
/** TUI 工具行 declutter 后 `.chat-tui-tool-one-line` 信号（human 名在旁，勿用 SSE summary 整句）。 */
const CREATE_HELLO_CPP_SIGNAL = "hello.cpp";

type DomOrder = {
  commentaryIdx: number;
  toolIdx: number;
  turnLabels: string[];
};

type OrderSample = DomOrder & {
  inverted: boolean;
  bothVisible: boolean;
  atMs: number;
};

function buildCommentaryThenToolSse(): string[] {
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
    next(COMMENTARY),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_segment_end",
        data: { segmentId: "seg-before-list" },
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
        toolCallId: TOOL_CALL_ID,
        name: TOOL_NAME,
        summary: TOOL_SUMMARY,
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_RESULT",
        toolCallId: TOOL_CALL_ID,
        content: ".",
        metadata: {
          name: TOOL_NAME,
          ok: true,
          summary: TOOL_SUMMARY,
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

function buildMidProcessSecondToolSse(): string[] {
  let id = 1;
  const next = (payload: string) => {
    const line = `id: ${id}\ndata: ${payload}\n\n`;
    id += 1;
    return line;
  };
  const firstCommentary = "先列一下目录。";
  const secondCommentary = "目录是空的，接下来创建 hello.cpp。";
  return [
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "assistant_answer_phase",
      }),
    ),
    next(firstCommentary),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "parsing_tool_calls",
        data: { parsing: true },
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_START",
        toolCallId: "tc_list",
        name: "list_tree",
        summary: "列出目录",
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_RESULT",
        toolCallId: "tc_list",
        content: ".",
        metadata: { name: "list_tree", ok: true, summary: "列出目录" },
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
        data: { segmentId: "seg-2", kind: "answer" },
      }),
    ),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "assistant_answer_phase",
      }),
    ),
    next(secondCommentary),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "parsing_tool_calls",
        data: { parsing: true },
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_START",
        toolCallId: "tc_create",
        name: "create_file",
        summary: "创建文件 hello.cpp",
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_RESULT",
        toolCallId: "tc_create",
        content: "ok",
        metadata: {
          name: "create_file",
          ok: true,
          summary: "创建文件 hello.cpp",
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
    next("已创建。"),
    next(JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: "1" })),
  ];
}

async function installOrderSampler(
  page: import("@playwright/test").Page,
  commentary: string,
  toolNeedle: string,
) {
  await page.evaluate(
    ({ commentaryText, toolText }) => {
      const w = window as unknown as {
        __cmOrderSamples?: OrderSample[];
        __cmOrderObserver?: MutationObserver;
      };
      w.__cmOrderSamples = [];
      const started = performance.now();
      const sample = () => {
        const turns = [
          ...document.querySelectorAll<HTMLElement>("section.chat-tui-turn"),
        ];
        const turnLabels = turns.map((el) => {
          const role = [...el.classList]
            .find((c) => c.startsWith("chat-tui-turn--"))
            ?.replace("chat-tui-turn--", "");
          const preview = (el.innerText ?? "")
            .replace(/\s+/g, " ")
            .trim()
            .slice(0, 40);
          return `${role}:${preview}`;
        });
        const commentaryIdx = turns.findIndex((el) => {
          if (!el.classList.contains("chat-tui-turn--assistant")) {
            return false;
          }
          const text = el.innerText ?? "";
          return text.includes(commentaryText);
        });
        const toolIdx = turns.findIndex((el) => {
          if (!el.classList.contains("chat-tui-turn--tool")) {
            return false;
          }
          return (el.innerText ?? "").includes(toolText);
        });
        const bothVisible = commentaryIdx >= 0 && toolIdx >= 0;
        w.__cmOrderSamples!.push({
          commentaryIdx,
          toolIdx,
          turnLabels,
          bothVisible,
          inverted: bothVisible && commentaryIdx > toolIdx,
          atMs: Math.round(performance.now() - started),
        });
      };
      sample();
      const root =
        document.querySelector('[data-testid="chat-tui-transcript"]') ??
        document.body;
      const observer = new MutationObserver(() => sample());
      observer.observe(root, {
        childList: true,
        subtree: true,
        characterData: true,
      });
      w.__cmOrderObserver = observer;
      // 定时补采，避免仅依赖 mutation 漏掉同帧写入
      const timer = window.setInterval(sample, 16);
      (w as unknown as { __cmOrderTimer?: number }).__cmOrderTimer =
        timer as unknown as number;
    },
    { commentaryText: commentary, toolText: toolNeedle },
  );
}

async function readOrderSamples(
  page: import("@playwright/test").Page,
): Promise<OrderSample[]> {
  return page.evaluate(() => {
    const w = window as unknown as { __cmOrderSamples?: OrderSample[] };
    return [...(w.__cmOrderSamples ?? [])];
  });
}

async function readCommentaryToolDomOrder(
  page: import("@playwright/test").Page,
  commentary: string,
  toolNeedle: string,
): Promise<DomOrder> {
  return page.evaluate(
    ({ commentaryText, toolText }) => {
      const turns = [
        ...document.querySelectorAll<HTMLElement>("section.chat-tui-turn"),
      ];
      const turnLabels = turns.map((el) => {
        const role = [...el.classList]
          .find((c) => c.startsWith("chat-tui-turn--"))
          ?.replace("chat-tui-turn--", "");
        const preview = (el.innerText ?? "")
          .replace(/\s+/g, " ")
          .trim()
          .slice(0, 40);
        return `${role}:${preview}`;
      });
      const commentaryIdx = turns.findIndex((el) => {
        if (!el.classList.contains("chat-tui-turn--assistant")) {
          return false;
        }
        const text = el.innerText ?? "";
        return text.includes(commentaryText);
      });
      const toolIdx = turns.findIndex((el) => {
        if (!el.classList.contains("chat-tui-turn--tool")) {
          return false;
        }
        return (el.innerText ?? "").includes(toolText);
      });
      return { commentaryIdx, toolIdx, turnLabels };
    },
    { commentaryText: commentary, toolText: toolNeedle },
  );
}

async function installDelayedSse(
  page: import("@playwright/test").Page,
  events: string[],
  conversationId: string,
) {
  await page.addInitScript(
    ({ sseEvents, delayMs, convId }) => {
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
              if (index >= sseEvents.length) {
                controller.close();
                return;
              }
              controller.enqueue(encoder.encode(sseEvents[index]));
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
    { sseEvents: events, delayMs: STREAM_DELAY_MS, convId: conversationId },
  );
}

test("during stream, pre-tool commentary must appear above its tool call", async ({
  page,
}) => {
  const chunks = buildCommentaryThenToolSse();
  const sid = `s_e2e_commentary_before_tool_${Date.now()}`;

  await installDelayedSse(page, chunks, CONV_ID);
  await seedSession(page, sid);
  await installOrderSampler(page, COMMENTARY, TOOL_SUMMARY);
  await sendMessage(page, "查看工作区结构");

  const transcript = page.getByTestId("chat-tui-transcript");

  // ① 旁白应先出现（工具尚未可见）
  await expect(transcript).toContainText(COMMENTARY, { timeout: 20_000 });
  const beforeTool = await readCommentaryToolDomOrder(
    page,
    COMMENTARY,
    TOOL_SUMMARY,
  );
  expect(
    beforeTool.commentaryIdx,
    `commentary missing before tool: ${JSON.stringify(beforeTool)}`,
  ).toBeGreaterThanOrEqual(0);
  expect(
    beforeTool.toolIdx,
    `tool must not appear before commentary finishes streaming: ${JSON.stringify(beforeTool)}`,
  ).toBe(-1);

  // ② 工具出现后立刻读序 + 采样历史不得出现反序
  await expect(transcript).toContainText(TOOL_SUMMARY, { timeout: 20_000 });
  const atToolVisible = await readCommentaryToolDomOrder(
    page,
    COMMENTARY,
    TOOL_SUMMARY,
  );
  expect(
    atToolVisible.commentaryIdx,
    `commentary missing when tool visible: ${JSON.stringify(atToolVisible)}`,
  ).toBeGreaterThanOrEqual(0);
  expect(
    atToolVisible.toolIdx,
    `tool missing: ${JSON.stringify(atToolVisible)}`,
  ).toBeGreaterThanOrEqual(0);
  expect(
    atToolVisible.commentaryIdx,
    `IMMEDIATE when tool visible: commentary must be above tool (got tool-first). turns=${JSON.stringify(atToolVisible.turnLabels)}`,
  ).toBeLessThan(atToolVisible.toolIdx);

  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 45_000,
  });

  const samples = await readOrderSamples(page);
  const bothVisible = samples.filter((s) => s.bothVisible);
  expect(
    bothVisible.length,
    "expected at least one sample with both commentary and tool visible",
  ).toBeGreaterThan(0);
  const inverted = bothVisible.filter((s) => s.inverted);
  expect(
    inverted,
    `stream order inverted (tool above commentary) in ${inverted.length} sample(s): ${JSON.stringify(inverted.slice(0, 5))}`,
  ).toEqual([]);

  const afterReady = await readCommentaryToolDomOrder(
    page,
    COMMENTARY,
    TOOL_SUMMARY,
  );
  expect(afterReady.commentaryIdx).toBeLessThan(afterReady.toolIdx);
});

test("mid-process second commentary must stay above its create tool during stream", async ({
  page,
}) => {
  const secondCommentary = "目录是空的，接下来创建 hello.cpp。";
  const chunks = buildMidProcessSecondToolSse();
  const sid = `s_e2e_mid_commentary_order_${Date.now()}`;

  await installDelayedSse(page, chunks, `${CONV_ID}-mid`);
  await seedSession(page, sid);
  await installOrderSampler(page, secondCommentary, CREATE_HELLO_CPP_SIGNAL);
  await sendMessage(page, "创建简单 c++ 文件");

  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(transcript).toContainText(secondCommentary, { timeout: 30_000 });
  await expect(transcript).toContainText(CREATE_HELLO_CPP_SIGNAL, {
    timeout: 20_000,
  });

  const atToolVisible = await readCommentaryToolDomOrder(
    page,
    secondCommentary,
    CREATE_HELLO_CPP_SIGNAL,
  );
  expect(atToolVisible.commentaryIdx).toBeGreaterThanOrEqual(0);
  expect(atToolVisible.toolIdx).toBeGreaterThanOrEqual(0);
  expect(
    atToolVisible.commentaryIdx,
    `mid-process IMMEDIATE: second commentary must be above create tool. turns=${JSON.stringify(atToolVisible.turnLabels)}`,
  ).toBeLessThan(atToolVisible.toolIdx);

  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 45_000,
  });

  const samples = await readOrderSamples(page);
  const inverted = samples.filter((s) => s.bothVisible && s.inverted);
  expect(
    inverted,
    `mid-process inverted samples: ${JSON.stringify(inverted.slice(0, 5))}`,
  ).toEqual([]);
});

test("when commentary arrives after tool_call, it must still render above the tool", async ({
  page,
}) => {
  // 形态 A：工具 SSE 先到，旁白晚到 — 用户仍期望「描述在工具之上」
  const lateCommentary = "工作区是空的。";
  const createSummary = "创建文件";
  let id = 1;
  const next = (payload: string) => {
    const line = `id: ${id}\ndata: ${payload}\n\n`;
    id += 1;
    return line;
  };
  const chunks = [
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "assistant_answer_phase",
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_START",
        toolCallId: "tc_read",
        name: "read_dir",
        summary: "列出目录",
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_RESULT",
        toolCallId: "tc_read",
        content: "empty",
        metadata: { name: "read_dir", ok: true, summary: "列出目录" },
      }),
    ),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_segment_start",
        data: {
          segmentId: "seg-before-tc_create",
          kind: "commentary",
          beforeToolCallId: "tc_create",
        },
      }),
    ),
    next(
      JSON.stringify({
        type: "TOOL_CALL_START",
        toolCallId: "tc_create",
        name: "create_file",
        summary: createSummary,
      }),
    ),
    // 旁白晚于 create 工具声明
    next(lateCommentary),
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
        toolCallId: "tc_create",
        content: "ok",
        metadata: {
          name: "create_file",
          ok: true,
          summary: createSummary,
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
        customType: "assistant_answer_phase",
      }),
    ),
    next("完成。"),
    next(JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: "1" })),
  ];

  const sid = `s_e2e_late_order_${Date.now()}`;
  await installDelayedSse(page, chunks, `${CONV_ID}-late`);
  await seedSession(page, sid);
  await installOrderSampler(page, lateCommentary, createSummary);
  await sendMessage(page, "在空工作区创建文件");

  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(transcript).toContainText(createSummary, { timeout: 20_000 });
  await expect(transcript).toContainText(lateCommentary, { timeout: 20_000 });
  const whenCommentaryArrives = await readCommentaryToolDomOrder(
    page,
    lateCommentary,
    createSummary,
  );
  expect(whenCommentaryArrives.commentaryIdx).toBeGreaterThanOrEqual(0);
  expect(whenCommentaryArrives.toolIdx).toBeGreaterThanOrEqual(0);
  expect(
    whenCommentaryArrives.commentaryIdx,
    `late commentary must jump above create tool immediately. turns=${JSON.stringify(whenCommentaryArrives.turnLabels)}`,
  ).toBeLessThan(whenCommentaryArrives.toolIdx);

  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 45_000,
  });
  const inverted = (await readOrderSamples(page)).filter(
    (s) => s.bothVisible && s.inverted,
  );
  expect(
    inverted,
    `late-commentary inverted while both visible: ${JSON.stringify(inverted.slice(0, 5))}`,
  ).toEqual([]);
});

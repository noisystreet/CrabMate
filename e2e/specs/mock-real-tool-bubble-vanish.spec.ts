/**
 * 由真实 LLM 红测 `real-llm-tool-bubble-vanish` 回放出来的两条「工具边界气泡闪没」时序。
 *
 * 共同根因：旁白一旦从 overlay 移交进 canonical，若此刻还没有工具锚点，
 * `project_turn_web_v2` 便投影不出 `turn-commentary-*` 行，而 overlay 已被清空 ——
 * 用户看到助手气泡整段消失，直到工具到达才恢复。
 *
 * A. `TOOL_CALL_RESULT` 无匹配 START
 *    → `on_tool_result_inserted` / `drain(clear=true)` 掏空 overlay，
 *      canonical 无该工具步 → pending 旁白锚不上。
 *
 * B. `turn_segment_start{beforeToolCallId}` 先于 `TOOL_CALL_START`（真实 SSE 的实际形态）
 *    → `reset_loading_tail_streaming_text` 清 overlay，
 *      pending 旁白要等 `ToolCall` 才被吸收，中间数百毫秒无可见行。
 *
 * 运行：
 *   cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/mock-real-tool-bubble-vanish.spec.ts
 */
import { expect, test, type Page } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const CHUNK_DELAY_MS = 40;
const PAINT_GAP_MS = 150;
/** 真实流里 `turn_segment_start` 到 `TOOL_CALL_START` 约 475ms。 */
const ANCHOR_GAP_MS = 400;
const SAMPLE_MS = 8;

const COMMENTARY = "我先看看当前目录的结构和源码文件。";
const COMMENTARY_PREFIX = COMMENTARY.slice(0, 18);
const FINAL_ANSWER = "目录里主要是 cpp-demo 与 package.json，源码很少。";

type VanishGap = {
  reason:
    | "assistant_body_sections_zero"
    | "seen_text_missing"
    | "seen_text_flicker_reappear";
  needle: string;
  tMs: number;
  bodyAssistantCount: number;
};

type MonitorResult = {
  gaps: VanishGap[];
  samples: number;
  firstBodyAtMs: number | null;
  maxBodyCount: number;
};

/** 一次 enqueue 的内容与其后的空档。 */
type StreamPhase = { data: string; delayMs: number };

function nextId(state: { id: number }, payload: string): string {
  const line = `id: ${state.id}\ndata: ${payload}\n\n`;
  state.id += 1;
  return line;
}

function chunkText(text: string, size: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < text.length; i += size) {
    out.push(text.slice(i, i + size));
  }
  return out.length ? out : [text];
}

/** `assistant_answer_phase` + 分块旁白，逐块留出绘制时间。 */
function commentaryPhases(state: { id: number }): StreamPhase[] {
  const phases: StreamPhase[] = [
    {
      data: nextId(
        state,
        JSON.stringify({
          type: "CUSTOM",
          customType: "assistant_answer_phase",
        }),
      ),
      delayMs: CHUNK_DELAY_MS,
    },
  ];
  for (const part of chunkText(COMMENTARY, 5)) {
    phases.push({ data: nextId(state, part), delayMs: CHUNK_DELAY_MS });
  }
  return phases;
}

function finalAnswerPhases(state: { id: number }): StreamPhase[] {
  return [
    {
      data: nextId(
        state,
        JSON.stringify({
          type: "CUSTOM",
          customType: "turn_segment_start",
          data: { segmentId: "seg-final", kind: "answer" },
        }),
      ),
      delayMs: CHUNK_DELAY_MS,
    },
    {
      data: nextId(
        state,
        JSON.stringify({
          type: "CUSTOM",
          customType: "assistant_answer_phase",
        }),
      ),
      delayMs: CHUNK_DELAY_MS,
    },
    { data: nextId(state, FINAL_ANSWER), delayMs: CHUNK_DELAY_MS },
    {
      data: nextId(
        state,
        JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: "1" }),
      ),
      delayMs: CHUNK_DELAY_MS,
    },
  ];
}

/** A：只发 `TOOL_CALL_RESULT`，不发 START（发了会留下 running 占位，状态栏卡住）。 */
function buildResultOnlyPhases(): StreamPhase[] {
  const state = { id: 1 };
  return [
    ...commentaryPhases(state),
    {
      data: nextId(
        state,
        JSON.stringify({
          type: "CUSTOM",
          customType: "parsing_tool_calls",
          data: { parsing: true },
        }),
      ),
      delayMs: PAINT_GAP_MS,
    },
    {
      data: [
        nextId(
          state,
          JSON.stringify({
            type: "TOOL_CALL_RESULT",
            toolCallId: "t-list-1",
            content: ".\n└── cpp-demo/\n",
            metadata: { name: "list_tree", ok: true, summary: "list tree: ." },
          }),
        ),
        nextId(
          state,
          JSON.stringify({
            type: "CUSTOM",
            customType: "turn_tool_phase_end",
            data: { phase: "tool_end" },
          }),
        ),
      ].join(""),
      delayMs: PAINT_GAP_MS,
    },
    ...finalAnswerPhases(state),
  ];
}

/** B：按真实 SSE 回放 —— 锚点段先声明，`TOOL_CALL_START` 数百毫秒后才到。 */
function buildSegmentAnchorPhases(): StreamPhase[] {
  const state = { id: 1 };
  const tcid = "call_00_w1SlVNO9N546tDtRA4Dj4107";
  const segmentId = `seg-before-${tcid}`;
  return [
    ...commentaryPhases(state),
    {
      data: [
        nextId(
          state,
          JSON.stringify({
            type: "CUSTOM",
            customType: "parsing_tool_calls",
            data: { parsing: true },
          }),
        ),
        nextId(
          state,
          JSON.stringify({
            type: "CUSTOM",
            customType: "turn_segment_start",
            data: {
              beforeToolCallId: tcid,
              kind: "commentary",
              segmentId,
            },
          }),
        ),
      ].join(""),
      // 关键采样窗口：此刻 overlay 已清，canonical 尚无工具步。
      delayMs: ANCHOR_GAP_MS,
    },
    {
      data: [
        nextId(
          state,
          JSON.stringify({
            type: "CUSTOM",
            customType: "turn_segment_end",
            data: { segmentId },
          }),
        ),
        nextId(
          state,
          JSON.stringify({
            type: "CUSTOM",
            customType: "tool_running",
            data: { running: true },
          }),
        ),
        nextId(
          state,
          JSON.stringify({
            type: "TOOL_CALL_START",
            toolCallId: tcid,
            name: "repo_overview_sweep",
            summary: "repo docs + tree + build files sweep",
          }),
        ),
        nextId(
          state,
          JSON.stringify({
            type: "TOOL_CALL_RESULT",
            toolCallId: tcid,
            content: "=== repo_overview_sweep（只读聚合）===\ncpp-demo/\n",
            metadata: {
              name: "repo_overview_sweep",
              ok: true,
              summary: "repo docs + tree + build files sweep",
            },
          }),
        ),
        nextId(
          state,
          JSON.stringify({
            type: "CUSTOM",
            customType: "turn_tool_phase_end",
            data: { phase: "tool_end" },
          }),
        ),
        nextId(
          state,
          JSON.stringify({
            type: "CUSTOM",
            customType: "tool_running",
            data: { running: false },
          }),
        ),
      ].join(""),
      delayMs: PAINT_GAP_MS,
    },
    ...finalAnswerPhases(state),
  ];
}

/** 装好 mock SSE 与闪没监控；监控经 `__cmInstallVanishMonitor` 手动启动。 */
async function installVanishHarness(
  page: Page,
  phases: StreamPhase[],
  convId: string,
) {
  await page.addInitScript(
    ({
      phases: streamPhases,
      convId: conversationId,
      sampleMs,
      commentaryPrefix,
    }) => {
      Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
        configurable: true,
        value: { invoke: () => Promise.resolve(null) },
      });

      const state = globalThis as typeof globalThis & {
        __cmStartedAt?: number;
        __cmInstallVanishMonitor?: () => void;
        __cmStopVanishMonitor?: () => MonitorResult;
      };

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
            let i = 0;
            const pump = () => {
              if (i >= streamPhases.length) {
                controller.close();
                return;
              }
              const phase = streamPhases[i];
              controller.enqueue(encoder.encode(phase.data));
              i += 1;
              window.setTimeout(pump, phase.delayMs);
            };
            pump();
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

      state.__cmInstallVanishMonitor = () => {
        const gaps: VanishGap[] = [];
        let samples = 0;
        let firstBodyAtMs: number | null = null;
        let maxBodyCount = 0;
        let frozenPrefix: string | null = null;
        let prefixGonePendingFlicker = false;
        let lastZeroGapAt = -1_000;
        let lastMissingGapAt = -1_000;

        const nowMs = () =>
          Math.round(
            performance.now() - (state.__cmStartedAt ?? performance.now()),
          );

        const sample = () => {
          samples += 1;
          const tMs = nowMs();
          const bodyAssistants = [
            ...document.querySelectorAll<HTMLElement>(
              "section.chat-tui-turn--assistant",
            ),
          ].filter((el) => {
            const text = (el.innerText ?? "").replace(/\s+/g, " ").trim();
            return text.length > 0 && !text.includes("意图分析");
          });
          maxBodyCount = Math.max(maxBodyCount, bodyAssistants.length);
          const transcriptText = (
            document.querySelector('[data-testid="chat-tui-transcript"]')
              ?.textContent ?? ""
          ).replace(/\s+/g, " ");

          if (bodyAssistants.length > 0 && frozenPrefix === null) {
            const hit = bodyAssistants.find((el) =>
              (el.innerText ?? "").includes(commentaryPrefix.slice(0, 8)),
            );
            const text = ((hit ?? bodyAssistants[0]).innerText ?? "")
              .replace(/\s+/g, " ")
              .trim();
            if (text.length >= 12) {
              frozenPrefix = text.slice(0, 18);
              firstBodyAtMs = tMs;
            }
          }
          if (firstBodyAtMs === null) return;

          if (bodyAssistants.length === 0 && tMs - lastZeroGapAt > 40) {
            gaps.push({
              reason: "assistant_body_sections_zero",
              needle: frozenPrefix ?? "*",
              tMs,
              bodyAssistantCount: 0,
            });
            lastZeroGapAt = tMs;
          }

          if (!frozenPrefix) return;
          if (transcriptText.includes(frozenPrefix)) {
            if (prefixGonePendingFlicker) {
              gaps.push({
                reason: "seen_text_flicker_reappear",
                needle: frozenPrefix,
                tMs,
                bodyAssistantCount: bodyAssistants.length,
              });
              prefixGonePendingFlicker = false;
            }
          } else if (tMs - lastMissingGapAt > 40) {
            prefixGonePendingFlicker = true;
            gaps.push({
              reason: "seen_text_missing",
              needle: frozenPrefix,
              tMs,
              bodyAssistantCount: bodyAssistants.length,
            });
            lastMissingGapAt = tMs;
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
        const timer = window.setInterval(sample, sampleMs);
        state.__cmStopVanishMonitor = () => {
          mo.disconnect();
          window.clearInterval(timer);
          return { gaps, samples, firstBodyAtMs, maxBodyCount };
        };
      };
    },
    {
      phases,
      convId,
      sampleMs: SAMPLE_MS,
      commentaryPrefix: COMMENTARY_PREFIX,
    },
  );
}

async function runVanishScenario(
  page: Page,
  phases: StreamPhase[],
  convId: string,
  sessionPrefix: string,
): Promise<MonitorResult> {
  await installVanishHarness(page, phases, convId);
  await seedSession(page, `${sessionPrefix}_${Date.now()}`);
  await expect(page.getByTestId("chat-tui-stream-view")).toBeVisible();
  await page.evaluate(() => {
    (
      globalThis as typeof globalThis & {
        __cmInstallVanishMonitor?: () => void;
      }
    ).__cmInstallVanishMonitor?.();
  });

  await sendMessage(page, "分析一下当前目录下的源码");

  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(transcript).toContainText(COMMENTARY_PREFIX.slice(0, 8), {
    timeout: 20_000,
  });
  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 30_000,
  });
  await expect(transcript).toContainText(FINAL_ANSWER.slice(0, 10), {
    timeout: 10_000,
  });
  await page.waitForTimeout(500);

  const result = await page.evaluate(() => {
    return (
      (
        globalThis as typeof globalThis & {
          __cmStopVanishMonitor?: () => MonitorResult;
        }
      ).__cmStopVanishMonitor?.() ?? null
    );
  });
  expect(result, "monitor must be installed").not.toBeNull();
  expect(result!.firstBodyAtMs, "commentary must paint").not.toBeNull();
  return result!;
}

/** 同 reason 的连续采样压成一条，便于阅读失败详情。 */
function compactGaps(gaps: VanishGap[]): VanishGap[] {
  return gaps.filter((gap, index, all) => {
    if (index === 0) return true;
    const prev = all[index - 1];
    return !(
      prev.reason === gap.reason &&
      prev.needle === gap.needle &&
      gap.tMs - prev.tMs < 120
    );
  });
}

function expectNoVanish(result: MonitorResult, label: string) {
  const compact = compactGaps(result.gaps);
  const detail = JSON.stringify({
    gaps: compact.slice(0, 20),
    firstBodyAtMs: result.firstBodyAtMs,
    maxBodyCount: result.maxBodyCount,
    samples: result.samples,
  });
  expect(
    compact.filter((g) => g.reason === "assistant_body_sections_zero"),
    `${label}: assistant body hit zero after commentary paint ${detail}`,
  ).toEqual([]);
  expect(compact, `${label}: commentary bubble vanished ${detail}`).toEqual([]);
}

test("tool_result without tool_call: assistant body must not hit zero", async ({
  page,
}) => {
  const result = await runVanishScenario(
    page,
    buildResultOnlyPhases(),
    "e2e-mock-vanish-result-only",
    "s_e2e_mock_vanish_result_only",
  );
  expectNoVanish(result, "result-only");
});

test("anchored segment before tool_call: assistant body must not hit zero", async ({
  page,
}) => {
  const result = await runVanishScenario(
    page,
    buildSegmentAnchorPhases(),
    "e2e-mock-vanish-segment-anchor",
    "s_e2e_mock_vanish_segment_anchor",
  );
  expectNoVanish(result, "segment-anchor");
});

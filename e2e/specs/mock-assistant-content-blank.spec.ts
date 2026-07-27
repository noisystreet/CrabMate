/**
 * 助手气泡仍在，但正文被掏空再刷回（同 data-tui-msg-id 的 body 变空）。
 *
 * 典型路径：旁白在 live loading 上 → 工具投影写出 commentary 行 → release_loading
 * 清空 live 正文 → 用户看见「内容没了」→ 终答再写入 live「又出现」。
 *
 * 运行：
 *   cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/mock-assistant-content-blank.spec.ts
 */
import { expect, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const STREAM_DELAY_MS = 100;
const CONV_ID = "e2e-assistant-content-blank";
const COMMENTARY =
  "好的，这是一个 C++ 项目，结构简洁。让我先快速浏览关键文件，了解项目全貌。";
const COMMENTARY_PREFIX = COMMENTARY.slice(0, 12);

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
        toolCallId: "t-read-1",
        name: "read_file",
        summary: "读取文件 README.md",
      }),
    ),
  );
  events.push(
    next(
      JSON.stringify({
        type: "TOOL_CALL_RESULT",
        toolCallId: "t-read-1",
        content: "ok",
        metadata: {
          name: "read_file",
          ok: true,
          summary: "read file: README.md",
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
  events.push(next("项目已全部读入。下面是分析：Hello CrabMate。"));
  events.push(
    next(JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: "1" })),
  );
  return events;
}

type BlankGap = {
  reason: string;
  msgId: string;
  tMs: number;
  chunkIndex: number;
  bodyLen: number;
};

test("live assistant body must not blank after commentary first paint", async ({
  page,
}) => {
  const sseChunks = buildSse();

  await page.addInitScript(
    ({ chunks, delayMs, convId, commentaryPrefix }) => {
      Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
        configurable: true,
        value: { invoke: () => Promise.resolve(null) },
      });

      const state = globalThis as typeof globalThis & {
        __cmChunkIndex?: number;
        __cmStartedAt?: number;
        __cmBlankGaps?: BlankGap[];
        __cmSamples?: number;
        __cmStopContentBlank?: () => {
          gaps: BlankGap[];
          samples: number;
          frozenId: string | null;
        };
      };
      state.__cmChunkIndex = -1;
      state.__cmBlankGaps = [];
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
          __cmInstallContentBlankMonitor?: () => void;
        }
      ).__cmInstallContentBlankMonitor = () => {
        const gaps: BlankGap[] = [];
        let frozenId: string | null = null;
        let sawCommentaryInFrozen = false;
        let markerSeen = false;
        let markerWasVisible = false;

        const sample = () => {
          state.__cmSamples = (state.__cmSamples ?? 0) + 1;
          const tMs = Math.round(
            performance.now() - (state.__cmStartedAt ?? performance.now()),
          );
          const chunkIndex = state.__cmChunkIndex ?? -1;

          // 冻结首次出现旁白的助手 section（通常是 live loading）
          if (!frozenId) {
            const hit = [
              ...document.querySelectorAll<HTMLElement>(
                "section.chat-tui-turn--assistant[data-tui-msg-id]",
              ),
            ].find((el) => (el.innerText ?? "").includes(commentaryPrefix));
            if (hit) {
              frozenId = hit.getAttribute("data-tui-msg-id");
              sawCommentaryInFrozen = true;
            }
          }

          if (frozenId) {
            const el = document.querySelector<HTMLElement>(
              `section.chat-tui-turn[data-tui-msg-id="${CSS.escape(frozenId)}"]`,
            );
            if (el) {
              const body =
                el.querySelector<HTMLElement>(".chat-tui-body") ?? el;
              const text = (body.innerText ?? "").replace(/\s+/g, " ").trim();
              const hasMarker = text.includes(commentaryPrefix);
              if (hasMarker) sawCommentaryInFrozen = true;
              // 同泡曾有旁白，随后 body 变空（气泡还在）
              if (sawCommentaryInFrozen && text.length === 0) {
                gaps.push({
                  reason: "frozen_section_body_blank",
                  msgId: frozenId,
                  tMs,
                  chunkIndex,
                  bodyLen: 0,
                });
              }
            }
          }

          // 全局旁白标记：出现过 → 消失 → 再出现（内容闪空）
          const allText = (
            document.querySelector<HTMLElement>(
              '[data-testid="chat-tui-transcript"]',
            )?.innerText ?? ""
          ).replace(/\s+/g, " ");
          const visible = allText.includes(commentaryPrefix);
          if (visible) {
            if (markerSeen && !markerWasVisible) {
              gaps.push({
                reason: "commentary_marker_reappear",
                msgId: frozenId ?? "",
                tMs,
                chunkIndex,
                bodyLen: -1,
              });
            }
            markerSeen = true;
            markerWasVisible = true;
          } else if (markerSeen) {
            markerWasVisible = false;
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
        state.__cmStopContentBlank = () => {
          mo.disconnect();
          window.clearInterval(timer);
          state.__cmBlankGaps = gaps;
          return { gaps, samples: state.__cmSamples ?? 0, frozenId };
        };
      };
    },
    {
      chunks: sseChunks,
      delayMs: STREAM_DELAY_MS,
      convId: CONV_ID,
      commentaryPrefix: COMMENTARY_PREFIX,
    },
  );

  await seedSession(page, `s_e2e_content_blank_${Date.now()}`);
  await expect(page.getByTestId("chat-tui-stream-view")).toBeVisible();
  await page.evaluate(() => {
    (
      globalThis as typeof globalThis & {
        __cmInstallContentBlankMonitor?: () => void;
      }
    ).__cmInstallContentBlankMonitor?.();
  });

  await sendMessage(page, "分析当前项目");
  const transcript = page.getByTestId("chat-tui-transcript");
  await expect(transcript).toContainText(COMMENTARY_PREFIX, {
    timeout: 20_000,
  });
  await expect(transcript).toContainText("README.md", { timeout: 20_000 });
  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 45_000,
  });
  await expect(transcript).toContainText("项目已全部读入", { timeout: 15_000 });
  await page.waitForTimeout(500);

  const result = await page.evaluate(() => {
    const state = globalThis as typeof globalThis & {
      __cmStopContentBlank?: () => {
        gaps: BlankGap[];
        samples: number;
        frozenId: string | null;
      };
    };
    return (
      state.__cmStopContentBlank?.() ?? {
        gaps: [],
        samples: 0,
        frozenId: null,
      }
    );
  });

  expect(
    result.frozenId,
    "should have frozen a commentary section",
  ).toBeTruthy();
  expect(
    result.gaps,
    `assistant content blanked: ${JSON.stringify(result.gaps)}; frozenId=${result.frozenId}; samples=${result.samples}`,
  ).toEqual([]);
  expect(result.samples).toBeGreaterThan(15);
});

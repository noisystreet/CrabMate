/**
 * 复现 Tauri/Web TUI 流式过程中「正文先出现 → 短暂消失 → 再出现」的闪烁。
 *
 * 监控目标：
 * 1. 工具边界后已定稿的 MARKER_A（ready 段）DOM 节点不得消失/清空；
 * 2. 流式 MARKER_B 一旦首次出现，采样期内不得出现「可见→不可见→可见」回跳；
 * 3. on_done 后延迟 hydrate（故意换 id / 扁平消息）不应造成 MARKER_A/B 文本空窗。
 *
 * 选择器走默认主列 TUI transcript（与 Tauri 同源），不用旧气泡 chat-message-row。
 */
import { expect, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const STREAM_CHUNK_DELAY_MS = 280;
const HYDRATE_DELAY_MS = 450;
const SAMPLE_MS = 8;
const MARKER_A = "MARKER_A_READY_SEGMENT";
const MARKER_B_PREFIX = "MARKER_B_LIVE";
const MARKER_B_FULL = "MARKER_B_LIVE_FINAL";
const CONV_ID = "e2e-tui-stream-flicker";

const SSE_CHUNKS = [
  'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
  `id: 2\ndata: ${MARKER_A}\n\n`,
  'id: 3\ndata: {"type":"TOOL_CALL_START","toolCallId":"t1","name":"list_tree","summary":"列出目录"}\n\n',
  'id: 4\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"t1","content":"empty","metadata":{"name":"list_tree","ok":true,"summary":"列出目录"}}\n\n',
  'id: 5\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
  'id: 6\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-2","kind":"answer"}}\n\n',
  'id: 7\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
  `id: 8\ndata: ${MARKER_B_PREFIX}\n\n`,
  `id: 9\ndata: _FINAL\n\n`,
  'id: 10\ndata: {"type":"RUN_FINISHED","threadId":"","runId":"1"}\n\n',
  'id: 11\ndata: {"type":"CUSTOM","customType":"conversation_saved","data":{"revision":1}}\n\n',
];

type FlickerGap = {
  marker: string;
  chunkIndex: number;
  tMs: number;
};

type DomDisappear = {
  id: string;
  marker: string;
  chunkIndex: number;
  tMs: number;
  kind: "missing" | "empty";
};

type FlickerMonitor = {
  freezeReady: (marker: string) => void;
  stop: () => {
    gaps: FlickerGap[];
    disappears: DomDisappear[];
    samples: Array<{
      tMs: number;
      a: boolean;
      b: boolean;
      chunkIndex: number;
    }>;
    fullRebuilds: number;
    frozen: Array<{ id: string; marker: string }>;
    hydrateFetches: number;
  };
};

test("TUI stream text must not disappear after first paint (incl. delayed hydrate)", async ({
  page,
}) => {
  await page.addInitScript(
    ({
      chunks,
      delayMs,
      hydrateDelayMs,
      convId,
      markerA,
      markerBFull,
      markerBPrefix,
      sampleMs,
    }) => {
      Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
        configurable: true,
        value: { invoke: () => Promise.resolve(null) },
      });

      const state = globalThis as typeof globalThis & {
        __cmChunkIndex?: number;
        __cmFlickerMonitor?: FlickerMonitor;
        __cmStreamStartedAt?: number;
        __cmHydrateFetches?: number;
      };
      state.__cmChunkIndex = -1;
      state.__cmStreamStartedAt = 0;
      state.__cmHydrateFetches = 0;

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

        if (url.includes("/conversation/messages") && method === "GET") {
          state.__cmHydrateFetches = (state.__cmHydrateFetches ?? 0) + 1;
          return new Promise<Response>((resolve) => {
            window.setTimeout(() => {
              resolve(
                new Response(
                  JSON.stringify({
                    conversation_id: convId,
                    // 必须 > conversation_saved 的 revision，才会触发 should_merge
                    revision: 2,
                    // 故意使用与本地投影不同的 id / 扁平结构，诱导 TUI full_html 重建
                    messages: [
                      {
                        role: "user",
                        content: "复现流式闪烁",
                        name: null,
                      },
                      {
                        role: "assistant",
                        content: markerA,
                        name: null,
                      },
                      {
                        role: "assistant",
                        content: "",
                        tool_calls: [
                          {
                            id: "t1",
                            type: "function",
                            function: {
                              name: "list_tree",
                              arguments: "{}",
                            },
                          },
                        ],
                      },
                      {
                        role: "tool",
                        content: "empty",
                        tool_call_id: "t1",
                        name: "list_tree",
                      },
                      {
                        role: "assistant",
                        content: markerBFull,
                        name: null,
                      },
                    ],
                    total_count: 5,
                    window_start_index: 0,
                    has_older: false,
                  }),
                  {
                    status: 200,
                    headers: { "content-type": "application/json" },
                  },
                ),
              );
            }, hydrateDelayMs);
          });
        }

        if (!url.includes("/chat/stream") || method !== "POST") {
          return originalFetch(input, init);
        }

        state.__cmStreamStartedAt = performance.now();
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

      // 安装闪烁监控（在 seed/reload 后 DOM 就绪时由测试调用 install）
      (
        globalThis as typeof globalThis & {
          __cmInstallFlickerMonitor?: () => void;
        }
      ).__cmInstallFlickerMonitor = () => {
        const MARKER_A_LOCAL = markerA;
        const MARKER_B_LOCAL = markerBPrefix;
        const gaps: FlickerGap[] = [];
        const disappears: DomDisappear[] = [];
        const samples: Array<{
          tMs: number;
          a: boolean;
          b: boolean;
          chunkIndex: number;
        }> = [];
        const frozen = new Map<string, string>();
        const frozenList: Array<{ id: string; marker: string }> = [];
        let aSeen = false;
        let aWasVisible = false;
        let bSeen = false;
        let bWasVisible = false;
        let fullRebuilds = 0;
        let sampleTimer = 0;

        const nowMs = () =>
          Math.round(performance.now() - (state.__cmStreamStartedAt || 0));

        const transcriptText = () => {
          const root =
            document.querySelector<HTMLElement>(
              '[data-testid="chat-tui-transcript"]',
            ) ?? document.querySelector<HTMLElement>(".messages-inner");
          return (root?.innerText ?? "").replace(/\s+/g, " ");
        };

        const findReadySection = (marker: string) => {
          const sections = [
            ...document.querySelectorAll<HTMLElement>(
              "section.chat-tui-turn[data-tui-msg-id]",
            ),
          ];
          return sections.find(
            (s) =>
              (s.innerText ?? "").includes(marker) &&
              s.getAttribute("data-tui-live") !== "1" &&
              !s.classList.contains("chat-tui-turn--loading"),
          );
        };

        const inspectFrozen = () => {
          for (const [id, marker] of frozen) {
            const el = document.querySelector<HTMLElement>(
              `section.chat-tui-turn[data-tui-msg-id="${id.replace(/"/g, '\\"')}"]`,
            );
            if (!el) {
              disappears.push({
                id,
                marker,
                chunkIndex: state.__cmChunkIndex ?? -1,
                tMs: nowMs(),
                kind: "missing",
              });
              continue;
            }
            const text = (el.innerText ?? "").replace(/\s+/g, " ").trim();
            if (!text.includes(marker)) {
              disappears.push({
                id,
                marker,
                chunkIndex: state.__cmChunkIndex ?? -1,
                tMs: nowMs(),
                kind: "empty",
              });
            }
          }
        };

        const track = (
          visible: boolean,
          seen: { v: boolean },
          was: { v: boolean },
          marker: string,
        ) => {
          if (visible) {
            if (seen.v && !was.v) {
              gaps.push({
                marker,
                chunkIndex: state.__cmChunkIndex ?? -1,
                tMs: nowMs(),
              });
            }
            seen.v = true;
            was.v = true;
          } else if (seen.v) {
            was.v = false;
          }
        };

        const aSeenBox = { v: false };
        const aWasBox = { v: false };
        const bSeenBox = { v: false };
        const bWasBox = { v: false };

        const sample = () => {
          const text = transcriptText();
          const a = text.includes(MARKER_A_LOCAL);
          const b = text.includes(MARKER_B_LOCAL);
          samples.push({
            tMs: nowMs(),
            a,
            b,
            chunkIndex: state.__cmChunkIndex ?? -1,
          });
          track(a, aSeenBox, aWasBox, MARKER_A_LOCAL);
          track(b, bSeenBox, bWasBox, MARKER_B_LOCAL);
          aSeen = aSeenBox.v;
          aWasVisible = aWasBox.v;
          bSeen = bSeenBox.v;
          bWasVisible = bWasBox.v;
          void aSeen;
          void aWasVisible;
          void bSeen;
          void bWasVisible;
          inspectFrozen();
        };

        const root =
          document.querySelector('[data-testid="chat-tui-transcript"]') ??
          document.querySelector(".messages-inner");
        if (!root) throw new Error("transcript root not found");

        // 拦截 transcript.innerHTML 赋值：full rebuild 若丢掉已出现正文即记 gap
        {
          const desc = Object.getOwnPropertyDescriptor(
            HTMLElement.prototype,
            "innerHTML",
          );
          if (desc?.set && desc.get) {
            let htmlHadA = false;
            let htmlHadB = false;
            Object.defineProperty(root, "innerHTML", {
              configurable: true,
              enumerable: true,
              get() {
                return desc.get!.call(this);
              },
              set(html: string) {
                const next = String(html ?? "");
                if (htmlHadA && !next.includes(MARKER_A_LOCAL)) {
                  gaps.push({
                    marker: `${MARKER_A_LOCAL}[innerHTML]`,
                    chunkIndex: state.__cmChunkIndex ?? -1,
                    tMs: nowMs(),
                  });
                }
                if (htmlHadB && !next.includes(MARKER_B_LOCAL)) {
                  gaps.push({
                    marker: `${MARKER_B_LOCAL}[innerHTML]`,
                    chunkIndex: state.__cmChunkIndex ?? -1,
                    tMs: nowMs(),
                  });
                }
                desc.set!.call(this, html);
                if (next.includes(MARKER_A_LOCAL)) htmlHadA = true;
                if (next.includes(MARKER_B_LOCAL)) htmlHadB = true;
              },
            });
          }
        }

        const mo = new MutationObserver((mutations) => {
          for (const m of mutations) {
            if (m.type === "childList" && m.removedNodes.length > 0) {
              if (
                [...m.removedNodes].some(
                  (n) =>
                    n instanceof HTMLElement &&
                    (n.matches?.("section.chat-tui-turn") ||
                      n.querySelector?.("section.chat-tui-turn")),
                )
              ) {
                fullRebuilds += 1;
              }
            }
          }
          sample();
        });
        mo.observe(root, {
          childList: true,
          characterData: true,
          subtree: true,
        });
        sampleTimer = window.setInterval(sample, sampleMs);

        state.__cmFlickerMonitor = {
          freezeReady(marker) {
            const section = findReadySection(marker);
            if (!section) throw new Error(`ready section not found: ${marker}`);
            const id = section.getAttribute("data-tui-msg-id");
            if (!id) throw new Error(`missing data-tui-msg-id for ${marker}`);
            if (!frozen.has(id)) {
              frozen.set(id, marker);
              frozenList.push({ id, marker });
            }
          },
          stop() {
            mo.disconnect();
            window.clearInterval(sampleTimer);
            return {
              gaps,
              disappears,
              samples,
              fullRebuilds,
              frozen: frozenList,
              hydrateFetches: state.__cmHydrateFetches ?? 0,
            };
          },
        };
      };
    },
    {
      chunks: SSE_CHUNKS,
      delayMs: STREAM_CHUNK_DELAY_MS,
      hydrateDelayMs: HYDRATE_DELAY_MS,
      convId: CONV_ID,
      markerA: MARKER_A,
      markerBFull: MARKER_B_FULL,
      markerBPrefix: MARKER_B_PREFIX,
      sampleMs: SAMPLE_MS,
    },
  );

  await seedSession(page, `s_e2e_tui_flicker_${Date.now()}`);
  await expect(page.getByTestId("chat-tui-stream-view")).toBeVisible();

  await page.evaluate(() => {
    const g = globalThis as typeof globalThis & {
      __cmInstallFlickerMonitor?: () => void;
    };
    g.__cmInstallFlickerMonitor?.();
  });

  await sendMessage(page, "复现流式闪烁");

  // MARKER_A 定稿后冻结 DOM id
  await page.waitForFunction(
    (marker) => {
      const sections = [
        ...document.querySelectorAll<HTMLElement>(
          "section.chat-tui-turn[data-tui-msg-id]",
        ),
      ];
      return sections.some(
        (s) =>
          (s.innerText ?? "").includes(marker) &&
          s.getAttribute("data-tui-live") !== "1",
      );
    },
    MARKER_A,
    { timeout: 20_000 },
  );
  await page.evaluate((marker) => {
    const state = globalThis as typeof globalThis & {
      __cmFlickerMonitor?: FlickerMonitor;
    };
    state.__cmFlickerMonitor?.freezeReady(marker);
  }, MARKER_A);

  await expect(page.getByTestId("chat-tui-transcript")).toContainText(
    MARKER_B_FULL,
    { timeout: 25_000 },
  );
  await expect(page.getByTestId("status-bar")).toContainText("就绪", {
    timeout: 20_000,
  });

  // 等待延迟 hydrate 完成后再采样几帧
  await page.waitForTimeout(HYDRATE_DELAY_MS + 800);
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );

  const result = await page.evaluate(() => {
    const state = globalThis as typeof globalThis & {
      __cmFlickerMonitor?: FlickerMonitor;
    };
    const monitor = state.__cmFlickerMonitor;
    if (!monitor) throw new Error("flicker monitor not installed");
    return monitor.stop();
  });

  const visibilityTrace = result.samples
    .filter((s) => s.a || s.b || s.chunkIndex >= 1)
    .filter((_, i) => i % 5 === 0)
    .slice(0, 50)
    .map((s) => `${s.tMs}ms#${s.chunkIndex}:A=${s.a ? 1 : 0}/B=${s.b ? 1 : 0}`)
    .join(" | ");

  expect(
    result.disappears,
    `ready DOM disappeared: ${JSON.stringify(result.disappears)}; rebuilds=${result.fullRebuilds}; hydrateFetches=${result.hydrateFetches}; trace=${visibilityTrace}`,
  ).toEqual([]);

  expect(
    result.gaps,
    `text flickered off after first paint: ${JSON.stringify(result.gaps)}; rebuilds=${result.fullRebuilds}; hydrateFetches=${result.hydrateFetches}; trace=${visibilityTrace}`,
  ).toEqual([]);

  expect(result.frozen.length).toBeGreaterThanOrEqual(1);
  // 确认 conversation_saved 后确实拉了水合（否则 hydrate 竞态未被覆盖）
  expect(
    result.hydrateFetches,
    `expected hydrate GET; rebuilds=${result.fullRebuilds}; trace=${visibilityTrace}`,
  ).toBeGreaterThanOrEqual(1);

  // 最终仍应可见两段正文
  await expect(page.getByTestId("chat-tui-transcript")).toContainText(MARKER_A);
  await expect(page.getByTestId("chat-tui-transcript")).toContainText(
    MARKER_B_FULL,
  );
});

test("harness detects forced transcript blanking (control)", async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
      configurable: true,
      value: { invoke: () => Promise.resolve(null) },
    });
  });
  await seedSession(page, `s_e2e_flicker_ctrl_${Date.now()}`);
  await expect(page.getByTestId("chat-tui-transcript")).toBeVisible();

  const forced = await page.evaluate(() => {
    const root = document.querySelector<HTMLElement>(
      '[data-testid="chat-tui-transcript"]',
    );
    if (!root) throw new Error("no transcript");
    const marker = "FORCED_MARKER_VISIBLE";
    root.innerHTML = `<section class="chat-tui-turn" data-tui-msg-id="x1">${marker}</section>`;
    const seen = root.innerText.includes(marker);
    root.innerHTML = `<section class="chat-tui-turn" data-tui-msg-id="x2"></section>`;
    const midMissing = !root.innerText.includes(marker);
    root.innerHTML = `<section class="chat-tui-turn" data-tui-msg-id="x3">${marker}</section>`;
    const back = root.innerText.includes(marker);
    return { seen, midMissing, back };
  });

  expect(forced.seen).toBe(true);
  expect(forced.midMissing).toBe(true);
  expect(forced.back).toBe(true);
});

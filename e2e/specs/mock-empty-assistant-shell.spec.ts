/**
 * 复现：消息正文未到时，空助手气泡（TUI 卡片 chrome）已出现。
 *
 * 根因假设：发送即 `push` Loading 空壳；主列 TUI 不经 ChatColumn 过滤，
 * 画出 `section.chat-tui-turn--assistant.is-loading` 且 `.chat-tui-body` 为空。
 *
 * 采样窗口：
 *   A. 发送后 → 首段正文 delta 前（首 token 空壳）
 *   B. 工具结果后 → 下一轮正文 delta 前（post-tool 新空尾）
 *
 * 期望：上述窗口内不得出现「空 body 的助手 turn」（TUI 跳过空 Loading 壳）。
 *
 * 运行：
 *   cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/mock-empty-assistant-shell.spec.ts
 */
import { expect, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const STREAM_DELAY_MS = 200;
const PRE_CONTENT_GAP_MS = 800;
const CONV_ID = "e2e-empty-assistant-shell";
const ANSWER = "工作区为空，可以开始。";
const POST_TOOL_ANSWER = "已创建 hello.cpp。";

type EmptyShellHit = {
  atMs: number;
  phase: string;
  msgId: string;
  bodyLen: number;
  className: string;
};

function sseLine(id: number, payload: string): string {
  return `id: ${id}\ndata: ${payload}\n\n`;
}

/** phase 后故意空档，再出正文，拉长「空壳可见」窗口。 */
function buildPreContentGapSse(): string[] {
  let id = 1;
  const next = (payload: string) => {
    const line = sseLine(id, payload);
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
    // SSE 注释帧：推进流但不写正文
    ": gap-1\n\n",
    ": gap-2\n\n",
    ": gap-3\n\n",
    next(ANSWER),
    next(JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: "1" })),
  ];
}

/** 工具结束后再空档，复现 post-tool 新 Loading 空尾。 */
function buildPostToolGapSse(): string[] {
  let id = 1;
  const next = (payload: string) => {
    const line = sseLine(id, payload);
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
    next("先看一下目录。"),
    next(
      JSON.stringify({
        type: "CUSTOM",
        customType: "turn_segment_end",
        data: { segmentId: "seg-before-list" },
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
        metadata: {
          name: "list_tree",
          ok: true,
          summary: "列出目录",
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
    ": gap-1\n\n",
    ": gap-2\n\n",
    ": gap-3\n\n",
    next(POST_TOOL_ANSWER),
    next(JSON.stringify({ type: "RUN_FINISHED", threadId: "", runId: "1" })),
  ];
}

async function installDelayedSseWithInitialGap(
  page: import("@playwright/test").Page,
  events: string[],
  options: {
    delayMs: number;
    initialGapMs: number;
    conversationId: string;
  },
) {
  await page.evaluate(
    ({ sseEvents, delayMs, initialGapMs, conversationId }) => {
      const originalFetch = window.fetch.bind(window);
      window.fetch = async (input, init) => {
        const requestUrl =
          typeof input === "string"
            ? input
            : input instanceof URL
              ? input.href
              : input.url;
        const method =
          init?.method ?? (input instanceof Request ? input.method : "GET");
        if (
          !requestUrl.includes("/chat/stream") ||
          method.toUpperCase() !== "POST"
        ) {
          return originalFetch(input, init);
        }

        const encoder = new TextEncoder();
        const body = new ReadableStream<Uint8Array>({
          async start(controller) {
            await new Promise((r) => window.setTimeout(r, initialGapMs));
            for (let i = 0; i < sseEvents.length; i += 1) {
              controller.enqueue(encoder.encode(sseEvents[i]));
              if (i + 1 < sseEvents.length) {
                await new Promise((r) => window.setTimeout(r, delayMs));
              }
            }
            controller.close();
          },
        });
        return new Response(body, {
          status: 200,
          headers: {
            "content-type": "text/event-stream; charset=utf-8",
            "x-conversation-id": conversationId,
            "x-stream-job-id": "1",
          },
        });
      };
    },
    {
      sseEvents: events,
      delayMs: options.delayMs,
      initialGapMs: options.initialGapMs,
      conversationId: options.conversationId,
    },
  );
}

async function installEmptyShellSampler(
  page: import("@playwright/test").Page,
): Promise<void> {
  await page.evaluate(() => {
    type Hit = {
      atMs: number;
      phase: string;
      msgId: string;
      bodyLen: number;
      className: string;
    };
    const w = window as unknown as {
      __emptyShellHits?: Hit[];
      __emptyShellPhase?: string;
      __emptyShellObserver?: MutationObserver;
    };
    w.__emptyShellHits = [];
    w.__emptyShellPhase = "pre_content";
    const started = performance.now();
    const sample = () => {
      const turns = document.querySelectorAll(
        "section.chat-tui-turn--assistant",
      );
      for (const turn of turns) {
        const body = turn.querySelector(".chat-tui-body");
        const text = (body?.textContent ?? "").replace(/\s+/g, " ").trim();
        if (text.length > 0) continue;
        const msgId = turn.getAttribute("data-tui-msg-id") ?? "";
        const last = w.__emptyShellHits![w.__emptyShellHits!.length - 1];
        if (
          last &&
          last.msgId === msgId &&
          last.phase === (w.__emptyShellPhase ?? "")
        ) {
          continue;
        }
        w.__emptyShellHits!.push({
          atMs: Math.round(performance.now() - started),
          phase: w.__emptyShellPhase ?? "",
          msgId,
          bodyLen: text.length,
          className: turn.className,
        });
      }
    };
    sample();
    const obs = new MutationObserver(() => sample());
    obs.observe(document.body, {
      childList: true,
      subtree: true,
      characterData: true,
    });
    w.__emptyShellObserver = obs;
    (
      window as unknown as { __emptyShellSample?: () => void }
    ).__emptyShellSample = sample;
  });
}

async function setSamplerPhase(
  page: import("@playwright/test").Page,
  phase: string,
) {
  await page.evaluate((p) => {
    (window as unknown as { __emptyShellPhase?: string }).__emptyShellPhase = p;
    (
      window as unknown as { __emptyShellSample?: () => void }
    ).__emptyShellSample?.();
  }, phase);
}

async function collectHits(
  page: import("@playwright/test").Page,
): Promise<EmptyShellHit[]> {
  return page.evaluate(() => {
    const w = window as unknown as {
      __emptyShellHits?: EmptyShellHit[];
      __emptyShellObserver?: MutationObserver;
    };
    w.__emptyShellObserver?.disconnect();
    return w.__emptyShellHits ?? [];
  });
}

test.describe("empty assistant shell before content", () => {
  test("发送后至首段正文前，不得出现空助手 TUI 卡", async ({ page }) => {
    await seedSession(page, `s_empty_shell_pre_${Date.now()}`);
    await installDelayedSseWithInitialGap(page, buildPreContentGapSse(), {
      delayMs: STREAM_DELAY_MS,
      initialGapMs: PRE_CONTENT_GAP_MS,
      conversationId: CONV_ID,
    });
    await installEmptyShellSampler(page);
    await sendMessage(page, "看一下工作区");

    // 首包延迟窗口内主动采样几次
    for (let i = 0; i < 4; i += 1) {
      await page.waitForTimeout(150);
      await page.evaluate(() =>
        (
          window as unknown as { __emptyShellSample?: () => void }
        ).__emptyShellSample?.(),
      );
    }

    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 30000 },
    );
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(ANSWER, { timeout: 5000 });

    const hits = await collectHits(page);
    const preHits = hits.filter((h) => h.phase === "pre_content");
    expect(
      preHits,
      `首 token 前出现空助手卡: ${JSON.stringify(preHits, null, 2)}`,
    ).toEqual([]);
  });

  test("工具结果后至下一轮正文前，不得出现空助手 TUI 卡", async ({ page }) => {
    await seedSession(page, `s_empty_shell_post_${Date.now()}`);
    await installDelayedSseWithInitialGap(page, buildPostToolGapSse(), {
      delayMs: STREAM_DELAY_MS,
      initialGapMs: 50,
      conversationId: `${CONV_ID}-post`,
    });
    await installEmptyShellSampler(page);
    await sendMessage(page, "列目录后创建文件");

    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText("列出目录", { timeout: 15000 });
    await setSamplerPhase(page, "post_tool");

    // 工具结果后 keep-alive 空档
    for (let i = 0; i < 6; i += 1) {
      await page.waitForTimeout(150);
      await page.evaluate(() =>
        (
          window as unknown as { __emptyShellSample?: () => void }
        ).__emptyShellSample?.(),
      );
    }

    await expect(page.locator('[data-testid="status-bar"]')).toContainText(
      "就绪",
      { timeout: 30000 },
    );
    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(POST_TOOL_ANSWER, { timeout: 5000 });

    const hits = await collectHits(page);
    const postHits = hits.filter((h) => h.phase === "post_tool");
    expect(
      postHits,
      `post-tool 空档出现空助手卡: ${JSON.stringify(postHits, null, 2)}`,
    ).toEqual([]);
  });
});

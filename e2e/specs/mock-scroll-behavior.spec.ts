import { expect, Page, test } from "@playwright/test";
import {
  installDelayedMockSse,
  seedSession,
  sendMessage,
  waitForText,
} from "../fixtures/helpers";

const FINAL_MARKER = "scroll-stream-final-marker";

async function seedScrollableSession(page: Page, sid: string, count = 50) {
  const messages = Array.from({ length: count }, (_, index) => ({
    id: `m_${index}`,
    role: index % 2 === 0 ? "user" : "assistant",
    text: `scroll-test-line-${index}`,
  }));
  await page.evaluate(
    ({ sessionId, seededMessages }) =>
      fetch("/user-data/workspaces/current/sessions", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          sessions: [
            {
              id: sessionId,
              title: "E2E scroll",
              draft: "",
              messages: seededMessages,
              updated_at: Date.now(),
              pinned: false,
              starred: false,
            },
          ],
          active_session_id: sessionId,
        }),
      }),
    { sessionId: sid, seededMessages: messages },
  );
  await page.reload({ waitUntil: "networkidle", timeout: 15_000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]');
}

async function prepareScrollableSession(page: Page, prefix: string) {
  const sid = `${prefix}_${Date.now()}`;
  await seedSession(page, sid);
  await seedScrollableSession(page, sid);
}

function buildLongSseEvents(chunkCount = 100): string[] {
  const events = [
    'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
  ];
  for (let index = 0; index < chunkCount; index += 1) {
    const suffix = index === chunkCount - 1 ? FINAL_MARKER : "";
    const chunkId = index.toString().padStart(3, "0");
    const delta =
      `stream-chunk-${chunkId}：这是一段用于验证长内容滚动稳定性的文本，` +
      `包含足够多的字符使消息区域持续增高。${suffix}\n`;
    events.push(
      `id: ${index + 2}\ndata: ${JSON.stringify({
        type: "TEXT_MESSAGE_CONTENT",
        delta,
      })}\n\n`,
    );
  }
  events.push(`id: ${chunkCount + 2}\ndata: {"type":"RUN_FINISHED"}\n\n`);
  return events;
}

async function scrollGapPx(page: Page): Promise<number> {
  return page.evaluate(() => {
    const element = document.querySelector(
      '[data-testid="chat-messages-scroller"]',
    );
    if (!element) return -1;
    return element.scrollHeight - element.scrollTop - element.clientHeight;
  });
}

async function waitForScrollToBottom(page: Page, timeoutMs = 5_000) {
  await page.waitForFunction(
    () => {
      const element = document.querySelector(
        '[data-testid="chat-messages-scroller"]',
      );
      if (!element) return false;
      const gap =
        element.scrollHeight - element.scrollTop - element.clientHeight;
      return gap >= 0 && gap <= 4;
    },
    undefined,
    { timeout: timeoutMs },
  );
}

async function scrollUp(page: Page, pixels = 320) {
  await page.evaluate((distance) => {
    const element = document.querySelector(
      '[data-testid="chat-messages-scroller"]',
    );
    if (!element) return;
    element.dispatchEvent(
      new WheelEvent("wheel", {
        deltaY: -distance,
        bubbles: true,
        cancelable: true,
      }),
    );
    element.scrollTop = Math.max(0, element.scrollTop - distance);
  }, pixels);
}

async function dragScrollbarUp(page: Page, pixels = 320) {
  await page.evaluate((distance) => {
    const element = document.querySelector(
      '[data-testid="chat-messages-scroller"]',
    );
    if (!element) return;
    element.dispatchEvent(
      new PointerEvent("pointerdown", {
        bubbles: true,
        button: 0,
        buttons: 1,
        pointerId: 1,
        isPrimary: true,
      }),
    );
    // 与 pointer 双保险：向上滚轮同步 unpin（Observe/指针竞态时仍关跟底）
    element.dispatchEvent(
      new WheelEvent("wheel", {
        deltaY: -distance,
        bubbles: true,
        cancelable: true,
      }),
    );
    element.scrollTop = Math.max(0, element.scrollTop - distance);
    element.dispatchEvent(new Event("scroll", { bubbles: true }));
  }, pixels);
  await page.waitForTimeout(50);
  await page.evaluate(() => {
    document
      .querySelector('[data-testid="chat-messages-scroller"]')
      ?.dispatchEvent(
        new PointerEvent("pointerup", {
          bubbles: true,
          button: 0,
          buttons: 0,
          pointerId: 1,
          isPrimary: true,
        }),
      );
  });
}

async function waitForStreamDone(page: Page) {
  await waitForText(page, FINAL_MARKER);
  await expect(page.locator('[data-testid="status-bar"]')).toContainText(
    "就绪",
  );
  await page.waitForTimeout(200);
}

test("长内容分批生成过程中持续贴底且终态 Markdown 不上跳", async ({ page }) => {
  await prepareScrollableSession(page, "s_e2e_scroll_long");
  await installDelayedMockSse(page, buildLongSseEvents(), 50);
  await sendMessage(page, "long streaming scroll test");

  await page.evaluate(() => {
    const samples: Array<{
      at: number;
      gap: number;
      scrollHeight: number;
      scrollTop: number;
    }> = [];
    const timer = window.setInterval(() => {
      const element = document.querySelector(
        '[data-testid="chat-messages-scroller"]',
      );
      if (element) {
        samples.push({
          at: performance.now(),
          gap: element.scrollHeight - element.scrollTop - element.clientHeight,
          scrollHeight: element.scrollHeight,
          scrollTop: element.scrollTop,
        });
      }
    }, 10);
    Object.assign(window, { __scrollProbe: { samples, timer } });
  });

  await waitForStreamDone(page);
  const samples = await page.evaluate(() => {
    const probe = (
      window as typeof window & {
        __scrollProbe?: {
          samples: Array<{
            at: number;
            gap: number;
            scrollHeight: number;
            scrollTop: number;
          }>;
          timer: number;
        };
      }
    ).__scrollProbe;
    if (!probe) return [];
    window.clearInterval(probe.timer);
    return probe.samples;
  });

  expect(samples.length).toBeGreaterThan(20);
  const first = samples[0]!;
  const last = samples.at(-1)!;
  expect(last.scrollHeight - first.scrollHeight).toBeGreaterThan(1_000);
  expect(last.scrollTop - first.scrollTop).toBeGreaterThan(1_000);
  const firstBottomIndex = samples.findIndex((sample) => sample.gap <= 4);
  expect(firstBottomIndex).toBeGreaterThanOrEqual(0);
  expect(samples[firstBottomIndex]!.at - first.at).toBeLessThan(500);
  const followingSamples = samples.slice(firstBottomIndex);
  let badGapStartedAt: number | undefined;
  let longestBadGapMs = 0;
  for (const sample of followingSamples) {
    if (sample.gap > 16) {
      badGapStartedAt ??= sample.at;
    } else if (badGapStartedAt !== undefined) {
      longestBadGapMs = Math.max(longestBadGapMs, sample.at - badGapStartedAt);
      badGapStartedAt = undefined;
    }
  }
  if (badGapStartedAt !== undefined) {
    longestBadGapMs = Math.max(longestBadGapMs, last.at - badGapStartedAt);
  }
  expect(longestBadGapMs).toBeLessThanOrEqual(200);
  await waitForScrollToBottom(page);
});

test("用户在生成期间上滚后不会被已排队的滚底任务拉回", async ({ page }) => {
  await prepareScrollableSession(page, "s_e2e_scroll_disengage");
  await installDelayedMockSse(page, buildLongSseEvents(), 30);
  await sendMessage(page, "disengage streaming scroll test");
  await waitForText(page, "stream-chunk-003：");
  await waitForScrollToBottom(page);

  await scrollUp(page);
  expect(await scrollGapPx(page)).toBeGreaterThan(100);
  await waitForStreamDone(page);

  expect(await scrollGapPx(page)).toBeGreaterThan(100);
});

test("用户在生成期间拖动滚动条离底后保持当前位置", async ({ page }) => {
  await prepareScrollableSession(page, "s_e2e_scroll_drag");
  await installDelayedMockSse(page, buildLongSseEvents(), 30);
  await sendMessage(page, "drag streaming scroll test");
  await waitForText(page, "stream-chunk-003：");
  await waitForScrollToBottom(page);

  await dragScrollbarUp(page);
  expect(await scrollGapPx(page)).toBeGreaterThan(100);
  await waitForStreamDone(page);

  expect(await scrollGapPx(page)).toBeGreaterThan(100);
});

test("生成期间手动滚回底部后 Observer 恢复自动跟随", async ({ page }) => {
  await prepareScrollableSession(page, "s_e2e_scroll_resume");
  await installDelayedMockSse(page, buildLongSseEvents(120), 30);
  await sendMessage(page, "resume streaming scroll test");
  await waitForText(page, "stream-chunk-003：");
  await waitForScrollToBottom(page);

  await scrollUp(page);
  expect(await scrollGapPx(page)).toBeGreaterThan(100);
  await page.evaluate(() => {
    const element = document.querySelector(
      '[data-testid="chat-messages-scroller"]',
    );
    if (element) element.scrollTop = element.scrollHeight;
  });
  await waitForScrollToBottom(page);
  await page.waitForTimeout(150);

  await waitForStreamDone(page);
  await waitForScrollToBottom(page);
});

test("生成完成后的延迟布局增高仍自动对齐最新消息末尾", async ({ page }) => {
  await prepareScrollableSession(page, "s_e2e_scroll_late_layout");
  await installDelayedMockSse(page, buildLongSseEvents(20), 20);
  await sendMessage(page, "late layout scroll test");
  await waitForStreamDone(page);
  await waitForScrollToBottom(page);

  const heightBefore = await page.evaluate(() => {
    const scroller = document.querySelector(
      '[data-testid="chat-messages-scroller"]',
    );
    const content = document.querySelector(".messages-inner");
    if (!scroller || !content) return -1;
    const before = scroller.scrollHeight;
    const spacer = document.createElement("div");
    spacer.dataset.testid = "late-layout-spacer";
    spacer.style.height = "600px";
    spacer.style.flex = "0 0 600px";
    content.append(spacer);
    return before;
  });

  expect(heightBefore).toBeGreaterThan(0);
  await page.waitForFunction((previousHeight) => {
    const scroller = document.querySelector(
      '[data-testid="chat-messages-scroller"]',
    );
    if (!scroller || scroller.scrollHeight < previousHeight + 500) {
      return false;
    }
    const gap =
      scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    return gap >= 0 && gap <= 4;
  }, heightBefore);
});

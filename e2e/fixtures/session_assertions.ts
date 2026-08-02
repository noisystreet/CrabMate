/**
 * 会话双写 / 导出一致性断言（流式本地窗 vs 重载后）。
 *
 * 覆盖缺口：`real-llm-bubble-layout` 会等 hydration 稳定后再比「重载前后一致」，
 * 会故意跳过「就绪瞬间本地双写」窗口（见 chat_export_210001 vs 210740）。
 * 本模块提供 **就绪后立刻** 可跑的检查，不依赖 server_revision 稳定。
 */
import { expect, type Page } from "@playwright/test";

export type PersistedMessage = {
  id?: string;
  role: string;
  text?: string;
  is_tool?: boolean;
};

export type PersistedSession = {
  id: string;
  messages?: PersistedMessage[];
  layout_schema_version?: number;
};

/** 从 sessions API 拉当前工作区会话（单次，不等 hydration 稳定）。 */
export async function fetchPersistedSession(
  page: Page,
  sid: string,
): Promise<PersistedSession | null> {
  return page.evaluate(async (sessionId) => {
    const response = await fetch("/user-data/workspaces/current/sessions");
    const data = await response.json();
    const list =
      (data.current?.sessions as PersistedSession[] | undefined) ??
      (data.sessions as PersistedSession[] | undefined) ??
      [];
    return list.find((session) => session.id === sessionId) ?? null;
  }, sid);
}

/** DOM：含 needle 的助手气泡。 */
export async function assistantDomHitsWithNeedle(
  page: Page,
  needle: string,
): Promise<string[]> {
  return page.evaluate((text) => {
    return [
      ...document.querySelectorAll<HTMLElement>(
        "section.chat-tui-turn--assistant",
      ),
    ]
      .map((el) => (el.innerText ?? "").replace(/\s+/g, " ").trim())
      .filter((t) => t.includes(text));
  }, needle);
}

/** 持久化：含 needle 的非工具助手行。 */
export function persistedAssistantHitsWithNeedle(
  messages: PersistedMessage[],
  needle: string,
): PersistedMessage[] {
  return messages.filter(
    (message) =>
      message.role === "assistant" &&
      !message.is_tool &&
      (message.text ?? "").includes(needle),
  );
}

/**
 * 模拟导出 Markdown 里「## 助手」段：每个非工具助手正文一段。
 * 与 Web display 导出同序；不跑完整展示过滤，只用于同文重复计数。
 */
export function markdownLikeAssistantBodies(
  messages: PersistedMessage[],
): string[] {
  return messages
    .filter((message) => message.role === "assistant" && !message.is_tool)
    .map((message) => (message.text ?? "").trim())
    .filter((text) => text.length > 0);
}

/** 相邻两条非工具助手正文完全相同（210001 典型成对双写）。 */
export function consecutiveDuplicateAssistantTexts(
  messages: PersistedMessage[],
): string[] {
  const bodies = markdownLikeAssistantBodies(messages);
  const dups: string[] = [];
  for (let i = 1; i < bodies.length; i++) {
    if (bodies[i] === bodies[i - 1]) {
      dups.push(bodies[i].slice(0, 48));
    }
  }
  return dups;
}

/** 就绪瞬间：DOM + 持久化 +「导出形」同文不得双份。 */
export async function assertNeedlesExactlyOncePreHydrate(args: {
  page: Page;
  sid: string;
  needles: readonly string[];
  label?: string;
}): Promise<void> {
  const tag = args.label ?? "pre-hydrate";
  await assertNeedlesDomExactlyOnce({
    page: args.page,
    needles: args.needles,
    label: tag,
  });

  const session = await fetchPersistedSession(args.page, args.sid);
  const messages = session?.messages ?? [];
  expect(messages.length, `[${tag}] session missing messages`).toBeGreaterThan(
    0,
  );

  const consecutive = consecutiveDuplicateAssistantTexts(messages);
  expect(
    consecutive,
    `[${tag}] consecutive identical assistant bodies (export-shaped pairs): ${JSON.stringify(consecutive)}`,
  ).toEqual([]);

  for (const needle of args.needles) {
    const rows = persistedAssistantHitsWithNeedle(messages, needle);
    expect(
      rows.length,
      `[${tag}] persisted count for ${needle.slice(0, 24)}… ids=${JSON.stringify(rows.map((r) => r.id))}`,
    ).toBe(1);

    const exportHits = markdownLikeAssistantBodies(messages).filter((body) =>
      body.includes(needle),
    );
    expect(
      exportHits.length,
      `[${tag}] export-shaped ##助手 sections for ${needle.slice(0, 24)}…`,
    ).toBe(1);
  }
}

/** 仅 DOM：各 needle 在助手气泡中恰好一条（流中采样用，不等持久化）。 */
export async function assertNeedlesDomExactlyOnce(args: {
  page: Page;
  needles: readonly string[];
  label: string;
}): Promise<void> {
  for (const needle of args.needles) {
    const domHits = await assistantDomHitsWithNeedle(args.page, needle);
    expect(
      domHits,
      `[${args.label}] DOM duplicate: ${needle.slice(0, 24)}… → ${JSON.stringify(domHits)}`,
    ).toHaveLength(1);
  }
}

export type StreamCommentarySampleStep = {
  /** 本步工具前旁白（出现后立刻采样）。 */
  commentary: string;
  /** 本步工具已可见的 DOM 文本（工具卡/摘要），出现后再采一次。 */
  afterToolText: string;
};

/**
 * 流中逐步采样：每段旁白出现后、对应工具出现后再断言
 * 「至此已见的全部旁白」DOM 各恰好 1；若 sessions 已有消息则顺带查相邻同文。
 *
 * 须在 delayed mock SSE（或真实慢流）下使用；一次性 dump 的 mock 可能跳过中间窗。
 */
export async function sampleCommentaryStepsDuringStream(args: {
  page: Page;
  sid: string;
  steps: readonly StreamCommentarySampleStep[];
  timeoutMs?: number;
}): Promise<void> {
  const timeout = args.timeoutMs ?? 30_000;
  const transcript = args.page.getByTestId("chat-tui-transcript");
  const seen: string[] = [];

  for (const [index, step] of args.steps.entries()) {
    await expect(transcript).toContainText(step.commentary, { timeout });
    seen.push(step.commentary);
    await assertNeedlesDomExactlyOnce({
      page: args.page,
      needles: seen,
      label: `stream-after-commentary-${index}`,
    });

    await expect(transcript).toContainText(step.afterToolText, { timeout });
    await assertNeedlesDomExactlyOnce({
      page: args.page,
      needles: seen,
      label: `stream-after-tool-${index}`,
    });

    const session = await fetchPersistedSession(args.page, args.sid);
    const messages = session?.messages ?? [];
    if (messages.length === 0) {
      continue;
    }
    const consecutive = consecutiveDuplicateAssistantTexts(messages);
    expect(
      consecutive,
      `[stream-after-tool-${index}] consecutive identical assistants: ${JSON.stringify(consecutive)}`,
    ).toEqual([]);
    for (const needle of seen) {
      const rows = persistedAssistantHitsWithNeedle(messages, needle);
      // 流中可能尚未 flush 到 sessions：0 可接受；≥2 则双写已落盘。
      expect(
        rows.length,
        `[stream-after-tool-${index}] persisted ${needle.slice(0, 24)}… count=${rows.length} ids=${JSON.stringify(rows.map((r) => r.id))}`,
      ).toBeLessThan(2);
    }
  }
}

/** 重载后再次断言：水合不得「修好双写」掩盖写路径问题（仍应恰好一条）。 */
export async function assertNeedlesExactlyOnceAfterReload(args: {
  page: Page;
  sid: string;
  needles: readonly string[];
}): Promise<void> {
  await args.page.reload({ waitUntil: "networkidle", timeout: 20_000 });
  await args.page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15_000,
  });
  await assertNeedlesExactlyOncePreHydrate({
    ...args,
    label: "post-reload",
  });
}

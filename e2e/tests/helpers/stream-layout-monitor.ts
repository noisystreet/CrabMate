/**
 * 真实 LLM / 长流式 Turn 布局：轮询聊天列块顺序，检测跳变与气泡消失。
 * 见 `docs/真实LLM-E2E.md` § 流式 DOM 监控。
 */
import { expect, type Page } from '@playwright/test';

import { visibleChatLayer } from './assertions';

/** 投影 stable id（与 `turn-batch-narration` / `turn-final-answer` 一致）。 */
export const STABLE_TURN_LAYOUT_ROW_IDS = ['turn-batch-narration', 'turn-final-answer'] as const;

export type TimelineMessageSlot = {
  id: string;
  /** 对应 DOM `.msg-loading`（流式尾泡/工具 loading 壳）。 */
  loading: boolean;
  /** DOM 含 `.msg-tool`（与 assistant 尾泡 loading 可并存）。 */
  isTool: boolean;
  /** DOM 含 `.msg-user`。 */
  isUser: boolean;
};

export type TimelineBlockSample = {
  t: number;
  /** `.messages-inner` 直接子块键（工具组为 `tg:`）。 */
  keys: string[];
  /** 块内全部 `msg-*` id（含工具组内各卡）。 */
  messages: TimelineMessageSlot[];
};

export type StreamLayoutViolationKind =
  | 'reorder'
  | 'vanished'
  | 'vanish_reappear'
  | 'stable_order_inversion'
  | 'committed_reorder'
  | 'multiple_loading'
  | 'loading_after_stream';

export type StreamLayoutViolation = {
  kind: StreamLayoutViolationKind;
  key: string;
  detail: string;
  sample_index: number;
};

export type StreamLayoutMonitorReport = {
  sample_count: number;
  /** 完整采样（仅内存；artifact 写入 excerpt）。 */
  samples: TimelineBlockSample[];
  violations: StreamLayoutViolation[];
  /** stable 行在各样本块序中的下标（-1 表示未出现）。 */
  stable_traces: Record<string, number[]>;
  /** 曾以非 loading 出现过的 message id（插入后不应消失）。 */
  committed_message_ids: string[];
};

export const REAL_LLM_STREAM_MONITOR = process.env.REAL_LLM_STREAM_MONITOR !== '0';
export const REAL_LLM_STRICT_STREAM_LAYOUT = process.env.REAL_LLM_STRICT_STREAM_LAYOUT === '1';
export const REAL_LLM_STREAM_POLL_MS = Number(process.env.REAL_LLM_STREAM_POLL_MS || 400);
const REAL_LLM_STREAM_TIMEOUT = Number(process.env.REAL_LLM_TIMEOUT || 300_000);

function isStableTurnRowId(id: string): boolean {
  return (STABLE_TURN_LAYOUT_ROW_IDS as readonly string[]).includes(id);
}

function messageIdsForBlockKey(key: string, sample: TimelineBlockSample): string[] {
  if (key.startsWith('tg:')) {
    return key
      .slice('tg:'.length)
      .split('+')
      .filter((id) => id && id !== '?');
  }
  if (isStableTurnRowId(key) || !key.startsWith('anon:')) {
    return [key];
  }
  return sample.messages.filter((m) => !m.loading).map((m) => m.id);
}

function blockKeyHasCommittedMessages(
  key: string,
  sample: TimelineBlockSample,
  committed: ReadonlySet<string>,
): boolean {
  return messageIdsForBlockKey(key, sample).some((id) => committed.has(id));
}

/** 块下标回跳是否仅因 ephemeral（未 commit）块消失。 */
function indexShrinkExplainedByEphemeralRemoval(
  newIdx: number,
  prevIdx: number,
  prevSample: TimelineBlockSample,
  committed: ReadonlySet<string>,
  skipBlockKeys: ReadonlySet<string>,
): boolean {
  if (newIdx >= prevIdx) {
    return false;
  }
  for (let i = newIdx; i < prevIdx; i++) {
    const blockKey = prevSample.keys[i];
    if (!blockKey || skipBlockKeys.has(blockKey)) {
      continue;
    }
    if (blockKeyHasCommittedMessages(blockKey, prevSample, committed)) {
      return false;
    }
  }
  return true;
}

function blockKeysForMessageId(sample: TimelineBlockSample, id: string): string[] {
  const keys: string[] = [];
  for (const key of sample.keys) {
    if (messageIdsForBlockKey(key, sample).includes(id)) {
      keys.push(key);
    }
  }
  return keys;
}

function blockIndexForMessageId(sample: TimelineBlockSample, id: string): number {
  const direct = sample.keys.indexOf(id);
  if (direct >= 0) {
    return direct;
  }
  for (let i = 0; i < sample.keys.length; i++) {
    const key = sample.keys[i]!;
    if (messageIdsForBlockKey(key, sample).includes(id)) {
      return i;
    }
  }
  return -1;
}

/** @deprecated 内部改用 {@link indexShrinkExplainedByEphemeralRemoval} */
function stableIndexShrinkExplainedByEphemeralRemoval(
  stableKey: string,
  newIdx: number,
  prevIdx: number,
  prevSample: TimelineBlockSample,
  committed: ReadonlySet<string>,
): boolean {
  return indexShrinkExplainedByEphemeralRemoval(
    newIdx,
    prevIdx,
    prevSample,
    committed,
    new Set([stableKey]),
  );
}

function slotCommitsOnSight(slot: TimelineMessageSlot): boolean {
  if (isStableTurnRowId(slot.id)) {
    return true;
  }
  if (slot.loading) {
    return false;
  }
  return slot.isUser || slot.isTool;
}

function isLikelyEmptyGlitchSample(sample: TimelineBlockSample): boolean {
  return sample.messages.length === 0 || sample.keys.every((k) => k.startsWith('anon:'));
}

function trimTrailingGlitchSamples(samples: TimelineBlockSample[]): void {
  while (samples.length > 1) {
    const last = samples[samples.length - 1]!;
    if (!isLikelyEmptyGlitchSample(last)) {
      break;
    }
    const prev = samples[samples.length - 2]!;
    if (isLikelyEmptyGlitchSample(prev)) {
      samples.pop();
      continue;
    }
    samples.pop();
    break;
  }
}

/** 采样失败或 Leptos 重绘瞬间：committed 全灭但非真实业务删除。 */
function isCorruptEmptySample(
  sample: TimelineBlockSample,
  committed: ReadonlySet<string>,
): boolean {
  if (committed.size === 0) {
    return false;
  }
  const present = new Set(sample.messages.map((m) => m.id));
  let anyCommittedPresent = false;
  for (const id of committed) {
    if (present.has(id)) {
      anyCommittedPresent = true;
      break;
    }
  }
  if (anyCommittedPresent) {
    return false;
  }
  return sample.messages.length === 0 || sample.keys.every((k) => k.startsWith('anon:'));
}

/** Turn 投影 resync 瞬间：stable 行与多条 committed 同时缺席，但 DOM 尚未清空。 */
function isMassCommittedVanishGlitch(
  sample: TimelineBlockSample,
  committed: ReadonlySet<string>,
): boolean {
  const present = new Set(sample.messages.map((m) => m.id));
  const stableCommitted = STABLE_TURN_LAYOUT_ROW_IDS.filter((id) => committed.has(id));
  if (stableCommitted.length === 0) {
    return false;
  }
  if (!stableCommitted.every((id) => !present.has(id))) {
    return false;
  }
  let missing = 0;
  for (const id of committed) {
    if (!present.has(id)) {
      missing++;
    }
  }
  return missing >= 2;
}

function shouldSkipVanishChecks(
  sample: TimelineBlockSample,
  committed: ReadonlySet<string>,
): boolean {
  return isCorruptEmptySample(sample, committed) || isMassCommittedVanishGlitch(sample, committed);
}

/** 采样聊天列：块序 + 块内全部消息 id（含 loading 态）。 */
export async function sampleTimelineSnapshot(page: Page): Promise<{
  keys: string[];
  messages: TimelineMessageSlot[];
}> {
  return visibleChatLayer(page)
    .locator('.messages-inner')
    .evaluate((el) => {
      const keys = Array.from(el.children).map((child) => {
        const msgEl = child.querySelector('[id^="msg-"]') as HTMLElement | null;
        if (msgEl?.id) {
          return msgEl.id.slice('msg-'.length);
        }
        const tools = child.querySelectorAll('[data-testid="chat-tool-card"]');
        if (tools.length > 0) {
          const ids = Array.from(tools)
            .map((t) => (t as HTMLElement).id?.slice('msg-'.length) || '?')
            .join('+');
          return `tg:${ids}`;
        }
        const snippet = (child.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 20);
        return snippet ? `anon:${snippet}` : 'anon:empty';
      });

      const messages: TimelineMessageSlot[] = [];
      for (const child of Array.from(el.children)) {
        for (const node of child.querySelectorAll('[id^="msg-"]')) {
          const html = node as HTMLElement;
          messages.push({
            id: html.id.slice('msg-'.length),
            loading: html.classList.contains('msg-loading'),
            isTool: html.classList.contains('msg-tool'),
            isUser: html.classList.contains('msg-user'),
          });
        }
      }
      return { keys, messages };
    });
}

/** @deprecated 使用 {@link sampleTimelineSnapshot} */
export async function sampleTimelineBlockKeys(page: Page): Promise<string[]> {
  const snap = await sampleTimelineSnapshot(page);
  return snap.keys;
}

export function analyzeTimelineSamples(samples: TimelineBlockSample[]): StreamLayoutMonitorReport {
  const violations: StreamLayoutViolation[] = [];
  const lastStableIndex = new Map<string, number>();
  const lastCommittedBlockIndex = new Map<string, number>();
  const stableTraces: Record<string, number[]> = Object.fromEntries(
    STABLE_TURN_LAYOUT_ROW_IDS.map((id) => [id, [] as number[]]),
  );
  /** 曾非 loading 落盘（或 stable 行）的 id：插入后不得从 DOM 消失。 */
  const committed = new Set<string>();
  /** 已报 `vanished` 且尚未 `vanish_reappear` 的 id。 */
  const absentCommitted = new Set<string>();

  const pushViolation = (v: StreamLayoutViolation) => {
    const dup = violations.some(
      (x) => x.kind === v.kind && x.key === v.key && x.sample_index === v.sample_index,
    );
    if (!dup) {
      violations.push(v);
    }
  };

  for (let si = 0; si < samples.length; si++) {
    const { keys, messages } = samples[si]!;
    const currentIds = new Set(messages.map((m) => m.id));
    const prevSample = si > 0 ? samples[si - 1]! : null;
    const corruptSample = shouldSkipVanishChecks(samples[si]!, committed);

    for (const stableId of STABLE_TURN_LAYOUT_ROW_IDS) {
      stableTraces[stableId]!.push(keys.indexOf(stableId));
    }

    const batchIdx = keys.indexOf('turn-batch-narration');
    const finalIdx = keys.indexOf('turn-final-answer');
    if (batchIdx >= 0 && finalIdx >= 0 && finalIdx < batchIdx) {
      pushViolation({
        kind: 'stable_order_inversion',
        key: 'turn-final-answer',
        detail: `turn-final-answer@${finalIdx} before turn-batch-narration@${batchIdx}`,
        sample_index: si,
      });
    }

    const assistantLoadingIds = messages
      .filter((m) => m.loading && !m.isTool)
      .map((m) => m.id);
    if (assistantLoadingIds.length > 1) {
      pushViolation({
        kind: 'multiple_loading',
        key: assistantLoadingIds.join('+'),
        detail: `${assistantLoadingIds.length} concurrent assistant .msg-loading: ${assistantLoadingIds.join(', ')}`,
        sample_index: si,
      });
    }

    if (!corruptSample) {
      for (const id of committed) {
        if (currentIds.has(id)) {
          if (absentCommitted.has(id)) {
            pushViolation({
              kind: 'vanish_reappear',
              key: id,
              detail: `${id} reappeared after vanishing (sample ${si})`,
              sample_index: si,
            });
            absentCommitted.delete(id);
          }
          continue;
        }
        if (!absentCommitted.has(id)) {
          pushViolation({
            kind: 'vanished',
            key: id,
            detail: `${id} missing from DOM after committed (sample ${si})`,
            sample_index: si,
          });
          absentCommitted.add(id);
        }
      }
    }

    for (let idx = 0; idx < keys.length; idx++) {
      const k = keys[idx]!;
      if (!isStableTurnRowId(k)) {
        continue;
      }
      if (lastStableIndex.has(k)) {
        const prev = lastStableIndex.get(k)!;
        if (idx < prev) {
          const explained =
            prevSample != null &&
            stableIndexShrinkExplainedByEphemeralRemoval(k, idx, prev, prevSample, committed);
          if (!explained) {
            pushViolation({
              kind: 'reorder',
              key: k,
              detail: `${k} block index ${prev} → ${idx} (backward jump)`,
              sample_index: si,
            });
          }
        }
      }
      lastStableIndex.set(k, idx);
    }

    for (const slot of messages) {
      if (slotCommitsOnSight(slot)) {
        committed.add(slot.id);
      }
    }

    if (!corruptSample) {
      for (const id of committed) {
        if (isStableTurnRowId(id)) {
          continue;
        }
        const idx = blockIndexForMessageId(samples[si]!, id);
        if (idx < 0) {
          continue;
        }
        if (lastCommittedBlockIndex.has(id)) {
          const prev = lastCommittedBlockIndex.get(id)!;
          if (idx < prev) {
            const skipKeys = new Set(blockKeysForMessageId(samples[si]!, id));
            const explained =
              prevSample != null &&
              indexShrinkExplainedByEphemeralRemoval(idx, prev, prevSample, committed, skipKeys);
            if (!explained) {
              pushViolation({
                kind: 'committed_reorder',
                key: id,
                detail: `${id} block index ${prev} → ${idx} (backward jump)`,
                sample_index: si,
              });
            }
          }
        }
        lastCommittedBlockIndex.set(id, idx);
      }
    }
  }

  const lastSample = samples[samples.length - 1];
  if (lastSample) {
    const lastSi = samples.length - 1;
    for (const slot of lastSample.messages) {
      if (slot.loading) {
        pushViolation({
          kind: 'loading_after_stream',
          key: slot.id,
          detail: `${slot.id} still .msg-loading in final snapshot`,
          sample_index: lastSi,
        });
      }
    }
  }

  return {
    sample_count: samples.length,
    samples,
    violations,
    stable_traces: stableTraces,
    committed_message_ids: [...committed].sort(),
  };
}

/** artifact 用：首尾 + 违规邻近样本，避免 JSON 过大。 */
export function excerptTimelineSamples(
  samples: TimelineBlockSample[],
  violations: StreamLayoutViolation[],
  max = 40,
): TimelineBlockSample[] {
  if (samples.length <= max) {
    return samples;
  }
  const pick = new Set<number>([0, samples.length - 1]);
  for (const v of violations) {
    for (let d = -2; d <= 2; d++) {
      const i = v.sample_index + d;
      if (i >= 0 && i < samples.length) {
        pick.add(i);
      }
    }
  }
  const sorted = [...pick].sort((a, b) => a - b);
  if (sorted.length <= max) {
    return sorted.map((i) => samples[i]!);
  }
  return sorted.slice(0, max).map((i) => samples[i]!);
}

export type StreamMonitorOptions = {
  pollMs?: number;
  /** 默认 REAL_LLM_STREAM_MONITOR；false 时不采样。 */
  enabled?: boolean;
};

/**
 * 发送消息并等待流结束；期间轮询 DOM 块顺序与消息 id。
 * `REAL_LLM_STRICT_STREAM_LAYOUT=1` 时 violations 非空则 throw。
 */
export async function sendAndWaitForStreamWithLayoutMonitor(
  page: Page,
  text: string,
  opts: StreamMonitorOptions = {},
): Promise<StreamLayoutMonitorReport | null> {
  const enabled = opts.enabled ?? REAL_LLM_STREAM_MONITOR;
  const pollMs = opts.pollMs ?? REAL_LLM_STREAM_POLL_MS;
  const samples: TimelineBlockSample[] = [];
  let polling = enabled;

  const pollLoop = (async () => {
    while (polling) {
      try {
        const snap = await sampleTimelineSnapshot(page);
        samples.push({ t: Date.now(), ...snap });
      } catch {
        // 页面/nav 切换时忽略单次采样失败
      }
      await page.waitForTimeout(pollMs);
    }
  })();

  const { fillComposerDraft } = await import('./composer');
  const streamDone = page.waitForResponse(
    (res) => res.url().includes('/chat/stream') && res.request().method() === 'POST',
    { timeout: REAL_LLM_STREAM_TIMEOUT },
  );
  await fillComposerDraft(page, text);
  await page.getByTestId('chat-send-button').click();
  const response = await streamDone;
  polling = false;
  await pollLoop;
  expect(response.ok(), await response.text()).toBeTruthy();
  await expect(page.getByTestId('chat-send-button')).toBeEnabled({ timeout: REAL_LLM_STREAM_TIMEOUT });
  await expect(page.getByRole('button', { name: '停止' })).toBeDisabled({
    timeout: REAL_LLM_STREAM_TIMEOUT,
  });

  if (!enabled) {
    return null;
  }

  await page.waitForTimeout(500);
  try {
    const snap = await sampleTimelineSnapshot(page);
    samples.push({ t: Date.now(), ...snap });
  } catch {
    // ignore
  }

  trimTrailingGlitchSamples(samples);

  const report = analyzeTimelineSamples(samples);
  if (REAL_LLM_STRICT_STREAM_LAYOUT && report.violations.length > 0) {
    throw new Error(
      `stream layout violations (${report.violations.length}): ${JSON.stringify(report.violations, null, 2)}`,
    );
  }
  return report;
}

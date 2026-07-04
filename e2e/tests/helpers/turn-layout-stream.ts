/**
 * Turn 布局 Phase 8：多工具 post-tool 块布局 SSE 桩（契约见 `fixtures/turn_project_web_golden.jsonl`）。
 */
import type { Page } from '@playwright/test';

import { installChatStreamStub } from './fix-chat-stream';
import { visibleChatLayer } from './assertions';

const SSE_V = 1;

export const TURN_LAYOUT_SESSION_ID = 's_e2e_turn_layout';

export const TURN_LAYOUT_USER_PROMPT = '编译 hpcg';

export const COMMENTARY_UNPACK = '好的，先解压。';
export const COMMENTARY_READ = '读取 INSTALL。';
export const COMMENTARY_MAKE = '开始编译。';

export const TOOL_ARCHIVE_UI = 'archive_list';
export const TOOL_UNPACK_UI = 'unpack';
export const TOOL_READ_UI = '读取文件';
export const TOOL_MAKE_UI = '命令执行';
/** Markdown 导出用工具详情标题（`run_command` + summary `ok` → `$ ok`）。 */
export const TOOL_MAKE_EXPORT = '$ ok';

export const FINAL_ANSWER = 'HPCG 编译流程结束。';

function sseEvent(id: number, data: string): string {
  return `id: ${id}\ndata: ${data}\n\n`;
}

function sseJson(payload: Record<string, unknown>): string {
  return JSON.stringify(payload);
}

function ssePreamble(id: number): { lines: string[]; nextId: number } {
  const lines = [
    sseEvent(id++, sseJson({ sse_capabilities: { supported_sse_v: SSE_V } })),
    sseEvent(id++, sseJson({ v: SSE_V })),
  ];
  return { lines, nextId: id };
}

function sseToolCall(toolCallId: string, name: string, summary: string): Record<string, unknown> {
  return {
    tool_call: {
      tool_call_id: toolCallId,
      name,
      summary,
    },
  };
}

function sseToolResult(toolCallId: string, name: string): Record<string, unknown> {
  return {
    tool_result: {
      tool_call_id: toolCallId,
      name,
      result_version: 1,
      summary: 'ok',
      output: 'e2e stub output',
      ok: true,
    },
  };
}

function sseSegmentStart(segmentId: string, beforeToolCallId: string): Record<string, unknown> {
  return {
    turn_segment_start: {
      segment_id: segmentId,
      kind: 'commentary',
      before_tool_call_id: beforeToolCallId,
    },
  };
}

function sseSegmentEnd(segmentId: string): Record<string, unknown> {
  return {
    turn_segment_end: {
      segment_id: segmentId,
    },
  };
}

export type TurnLayoutStreamOptions = {
  /** `segment_end` 早于 `tool_call`（测 overlay 保留与 flush 重试）。 */
  segmentEndBeforeUnpackTool?: boolean;
  conversationId?: string;
};

/** 精简 HPCG 链：archive → (旁注+unpack) → (旁注+read) → (旁注+make) → 终答 */
export function buildTurnLayoutInterleavedStreamBody(
  opts: TurnLayoutStreamOptions = {},
): string {
  const { lines, nextId: startId } = ssePreamble(1);
  let id = startId;

  lines.push(sseEvent(id++, sseJson(sseToolCall('tc_archive', 'archive_list', 'list archive'))));
  lines.push(sseEvent(id++, sseJson(sseToolResult('tc_archive', 'archive_list'))));

  lines.push(
    sseEvent(
      id++,
      sseJson(sseSegmentStart('seg-before-tc_unpack', 'tc_unpack')),
    ),
  );
  lines.push(sseEvent(id++, COMMENTARY_UNPACK));
  if (opts.segmentEndBeforeUnpackTool) {
    lines.push(
      sseEvent(id++, sseJson(sseSegmentEnd('seg-before-tc_unpack'))),
    );
  }
  lines.push(sseEvent(id++, sseJson(sseToolCall('tc_unpack', 'unpack', 'unpack archive'))));
  lines.push(sseEvent(id++, sseJson(sseToolResult('tc_unpack', 'unpack'))));

  lines.push(
    sseEvent(id++, sseJson(sseSegmentStart('seg-before-tc_read', 'tc_read'))),
  );
  lines.push(sseEvent(id++, COMMENTARY_READ));
  lines.push(sseEvent(id++, sseJson(sseToolCall('tc_read', 'read_file', 'read INSTALL'))));
  lines.push(sseEvent(id++, sseJson(sseToolResult('tc_read', 'read_file'))));

  lines.push(
    sseEvent(id++, sseJson(sseSegmentStart('seg-before-tc_make', 'tc_make'))),
  );
  lines.push(sseEvent(id++, COMMENTARY_MAKE));
  lines.push(sseEvent(id++, sseJson(sseToolCall('tc_make', 'run_command', 'make'))));
  lines.push(sseEvent(id++, sseJson(sseToolResult('tc_make', 'run_command'))));

  lines.push(sseEvent(id++, sseJson({ turn_tool_phase_end: true })));
  lines.push(sseEvent(id++, FINAL_ANSWER));
  lines.push(sseEvent(id++, sseJson({ stream_ended: { reason: 'completed' } })));

  return lines.join('');
}

export async function installTurnLayoutStreamStub(
  page: Page,
  opts: TurnLayoutStreamOptions = {},
): Promise<void> {
  await installChatStreamStub(page, {
    conversationId: opts.conversationId ?? 'e2e-turn-layout-conv',
    body: buildTurnLayoutInterleavedStreamBody(opts),
  });
}

/** 聊天列时间线块（含助手行与工具卡）自上而下 innerText。 */
export async function visibleTimelineBlockTexts(page: Page): Promise<string[]> {
  return visibleChatLayer(page).locator('.messages-inner').evaluate((el) => {
    return Array.from(el.children).map((c) => (c as HTMLElement).innerText);
  });
}

export function indexOfRowContaining(texts: string[], needle: string): number {
  return texts.findIndex((t) => t.includes(needle));
}

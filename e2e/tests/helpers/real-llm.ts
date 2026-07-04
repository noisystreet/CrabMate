/**
 * 真实 LLM E2E：流式等待、Turn 布局导出断言、失败 artifact 落盘。
 * 见 `docs/真实LLM-E2E.md`。
 */
import { expect, type APIRequestContext, type Page, type TestInfo } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';

import { UI_TIMEOUT } from './assertions';
import { fillComposerDraft } from './composer';
import {
  countMarkdownAssistantSections,
  exportSessionJsonFromModal,
  exportSessionMarkdownFromModal,
} from './phase5-visible-messages';

export const REAL_LLM_ENABLED = process.env.REAL_LLM_E2E === '1';
export const REAL_LLM_CAPTURE = process.env.REAL_LLM_CAPTURE === '1';
export const REAL_LLM_TIMEOUT = Number(process.env.REAL_LLM_TIMEOUT || 300_000);
export const REAL_LLM_WORKSPACE = process.env.REAL_LLM_WORKSPACE || '/home/gzz/test';
/** 单节 assistant 超过此长度时在 report 中标记 megaBubbleSuspected。 */
export const REAL_LLM_MEGA_BUBBLE_CHARS = Number(process.env.REAL_LLM_MEGA_BUBBLE_CHARS || 2500);

export const REAL_LLM_SESSION_ANALYZE = 's_e2e_real_turn_analyze';
export const REAL_LLM_SESSION_COMPILE = 's_e2e_real_turn_compile';
export const REAL_LLM_SESSION_FULL = 's_e2e_real_turn_layout';

export type WorkspaceHints = {
  workspace: string;
  has_hpcg_tar: boolean;
  has_xhpcg: boolean;
  hpcg_tar_names: string[];
};

export type RealLlmMeta = {
  e2e_port: string;
  user_data_dir: string | null;
  health_status: string | null;
  api_key_check_ok: boolean | null;
  model: string | null;
  api_base: string | null;
};

export type TurnLayoutCompileReport = {
  compile_turn_found: boolean;
  tool_section_found: boolean;
  before_tool_assistant_count: number;
  after_tool_assistant_count: number;
  before_tool_preview: string;
  after_tool_preview: string;
  max_assistant_section_chars: number;
  mega_bubble_suspected: boolean;
  assistant_section_count: number;
};

export type RealLlmArtifactBundle = {
  sessionId: string;
  exportMd?: string;
  exportJson?: unknown;
  compileReport?: TurnLayoutCompileReport;
  errorMessage?: string;
};

function repoRootFromE2e(): string {
  return path.resolve(__dirname, '..', '..', '..');
}

function artifactsRoot(): string {
  return path.join(repoRootFromE2e(), 'e2e', 'artifacts', 'real-llm');
}

function excerpt(text: string, max = 120): string {
  const t = text.replace(/\s+/g, ' ').trim();
  return t.length <= max ? t : `${t.slice(0, max)}…`;
}

function assistantBodies(md: string): string[] {
  return [...md.matchAll(/## 助手\n\n([\s\S]*?)(?=\n## |$)/g)].map((m) => m[1].trim());
}

function maxSectionChars(md: string): number {
  return assistantBodies(md).reduce((max, body) => Math.max(max, body.length), 0);
}

/** 绑定工作区；仅 log 前置条件，不因已编译而失败。 */
export async function setupRealLlmWorkspace(
  request: APIRequestContext,
  workspace = REAL_LLM_WORKSPACE,
): Promise<WorkspaceHints> {
  const setWs = await request.post('/workspace', { data: { path: workspace } });
  expect(setWs.ok(), await setWs.text()).toBeTruthy();
  const hints = probeWorkspaceHints(workspace);
  console.log('[real-llm] workspace hints:', JSON.stringify(hints));
  return hints;
}

export function probeWorkspaceHints(workspace: string): WorkspaceHints {
  let hpcgTarNames: string[] = [];
  try {
    hpcgTarNames = fs
      .readdirSync(workspace)
      .filter((name) => /hpcg/i.test(name) && /\.tar\.gz$/i.test(name));
  } catch {
    // 目录不可读时仅报告 false
  }
  const xhpcgCandidates = [
    path.join(workspace, 'bin', 'xhpcg'),
    path.join(workspace, 'hpcg', 'bin', 'xhpcg'),
  ];
  return {
    workspace,
    has_hpcg_tar: hpcgTarNames.length > 0,
    has_xhpcg: xhpcgCandidates.some((p) => fs.existsSync(p)),
    hpcg_tar_names: hpcgTarNames,
  };
}

export async function fetchRealLlmMeta(request: APIRequestContext): Promise<RealLlmMeta> {
  const meta: RealLlmMeta = {
    e2e_port: process.env.E2E_PORT || '18081',
    user_data_dir: process.env.CM_CRABMATE_USER_DATA_DIR || null,
    health_status: null,
    api_key_check_ok: null,
    model: null,
    api_base: null,
  };
  try {
    const health = await request.get('/health');
    if (health.ok()) {
      const body = (await health.json()) as {
        status?: string;
        checks?: { api_key?: { ok?: boolean } };
      };
      meta.health_status = body.status ?? null;
      meta.api_key_check_ok = body.checks?.api_key?.ok ?? null;
    }
  } catch {
    // 手动 serve 未起时跳过
  }
  try {
    const status = await request.get('/status');
    if (status.ok()) {
      const body = (await status.json()) as { model?: string; api_base?: string };
      meta.model = body.model ?? null;
      meta.api_base = body.api_base ?? null;
    }
  } catch {
    // ignore
  }
  return meta;
}

export async function sendAndWaitForStream(page: Page, text: string): Promise<void> {
  const streamDone = page.waitForResponse(
    (res) => res.url().includes('/chat/stream') && res.request().method() === 'POST',
    { timeout: REAL_LLM_TIMEOUT },
  );
  await fillComposerDraft(page, text);
  await page.getByTestId('chat-send-button').click();
  const response = await streamDone;
  expect(response.ok(), await response.text()).toBeTruthy();
  await expect(page.getByTestId('chat-send-button')).toBeEnabled({ timeout: REAL_LLM_TIMEOUT });
  await expect(page.getByRole('button', { name: '停止' })).toBeDisabled({ timeout: REAL_LLM_TIMEOUT });
}

export async function gotoCrabMateHome(page: Page): Promise<void> {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'CrabMate' })).toBeVisible({
    timeout: UI_TIMEOUT,
  });
}

/** 分析 compile 轮在导出 MD 中的布局（不 throw）。 */
export function analyzeCompileTurnLayout(md: string): TurnLayoutCompileReport {
  const assistantSectionCount = countMarkdownAssistantSections(md);
  const compileTurnStart = md.search(/## 用户\n\n[^\n]*编译[^\n]*hpcg/i);
  if (compileTurnStart < 0) {
    return {
      compile_turn_found: false,
      tool_section_found: false,
      before_tool_assistant_count: 0,
      after_tool_assistant_count: 0,
      before_tool_preview: '',
      after_tool_preview: '',
      max_assistant_section_chars: maxSectionChars(md),
      mega_bubble_suspected: maxSectionChars(md) > REAL_LLM_MEGA_BUBBLE_CHARS,
      assistant_section_count: assistantSectionCount,
    };
  }
  const turnSlice = md.slice(compileTurnStart);
  const toolIdxInTurn = turnSlice.search(/^## 工具\n\n/m);
  const beforeTools = toolIdxInTurn >= 0 ? turnSlice.slice(0, toolIdxInTurn) : turnSlice;
  const afterTools = toolIdxInTurn >= 0 ? turnSlice.slice(toolIdxInTurn) : '';
  const beforeBodies = [
    ...beforeTools.matchAll(/## 助手\n\n([\s\S]*?)(?=\n## |$)/g),
  ].map((m) => m[1].trim());
  const afterBodies = [
    ...afterTools.matchAll(/## 助手\n\n([\s\S]*?)(?=\n## |$)/g),
  ].map((m) => m[1].trim());
  const maxChars = Math.max(maxSectionChars(md), ...beforeBodies.map((b) => b.length), ...afterBodies.map((b) => b.length));
  return {
    compile_turn_found: true,
    tool_section_found: toolIdxInTurn >= 0,
    before_tool_assistant_count: beforeBodies.length,
    after_tool_assistant_count: afterBodies.length,
    before_tool_preview: excerpt(beforeBodies[0] ?? ''),
    after_tool_preview: excerpt(afterBodies[0] ?? ''),
    max_assistant_section_chars: maxChars,
    mega_bubble_suspected: maxChars > REAL_LLM_MEGA_BUBBLE_CHARS,
    assistant_section_count: assistantSectionCount,
  };
}

/** 编译轮：batch + final 分节，非巨泡。失败时 error 含 report JSON。 */
export function assertCompileTurnLayoutExport(md: string): TurnLayoutCompileReport {
  const report = analyzeCompileTurnLayout(md);
  const errors: string[] = [];
  if (report.assistant_section_count < 2) {
    errors.push(`assistant sections >= 2 (got ${report.assistant_section_count})`);
  }
  if (!report.compile_turn_found) {
    errors.push('compile user turn not found in export');
  }
  if (!report.tool_section_found) {
    errors.push('## 工具 section missing in compile turn');
  }
  if (report.before_tool_assistant_count < 1) {
    errors.push('need >=1 assistant section before tools in compile turn');
  }
  if (report.after_tool_assistant_count < 1) {
    errors.push('need >=1 assistant section after tools in compile turn');
  }
  if (report.before_tool_preview.length <= 10) {
    errors.push('before-tool assistant too short');
  }
  if (!/编译|xhpcg|HPCG|总结|完成|成功|make/i.test(report.after_tool_preview)) {
    errors.push('after-tool assistant missing compile/final keywords');
  }
  if (
    report.before_tool_preview &&
    report.after_tool_preview &&
    report.before_tool_preview === report.after_tool_preview
  ) {
    errors.push('before/after tool assistant previews must differ (mega bubble?)');
  }
  if (report.mega_bubble_suspected) {
    errors.push(`mega bubble suspected (section chars > ${REAL_LLM_MEGA_BUBBLE_CHARS})`);
  }
  if (errors.length > 0) {
    throw new Error(`${errors.join('; ')}\n\nturn-layout-report:\n${JSON.stringify(report, null, 2)}`);
  }
  return report;
}

export async function exportSessionArtifacts(
  page: Page,
  sessionId: string,
): Promise<{ md: string; json: unknown }> {
  const md = await exportSessionMarkdownFromModal(page, sessionId);
  let json: unknown = null;
  try {
    json = await exportSessionJsonFromModal(page, sessionId);
  } catch (err) {
    console.warn('[real-llm] JSON export skipped:', err);
  }
  return { md, json };
}

function slugify(title: string): string {
  return title.replace(/[^\w\u4e00-\u9fff-]+/g, '-').slice(0, 60);
}

/** 失败或 REAL_LLM_CAPTURE=1 时写入 `e2e/artifacts/real-llm/<run>/`。 */
export async function writeRealLlmArtifacts(
  testInfo: TestInfo,
  request: APIRequestContext,
  bundle: RealLlmArtifactBundle,
  workspaceHints?: WorkspaceHints,
): Promise<string | null> {
  const shouldWrite = REAL_LLM_CAPTURE || testInfo.status !== 'passed';
  if (!shouldWrite) {
    return null;
  }
  const runDir = path.join(
    artifactsRoot(),
    `${new Date().toISOString().replace(/[:.]/g, '-')}_${slugify(testInfo.title)}`,
  );
  fs.mkdirSync(runDir, { recursive: true });

  const meta = await fetchRealLlmMeta(request);
  const payload = {
    test: {
      title: testInfo.title,
      file: testInfo.file,
      status: testInfo.status,
      duration_ms: testInfo.duration,
      error: testInfo.error?.message ?? bundle.errorMessage ?? null,
    },
    meta,
    workspace_hints: workspaceHints ?? probeWorkspaceHints(REAL_LLM_WORKSPACE),
    session_id: bundle.sessionId,
    turn_layout: bundle.compileReport ?? null,
  };

  fs.writeFileSync(path.join(runDir, 'meta.json'), JSON.stringify(payload, null, 2));
  if (bundle.compileReport) {
    fs.writeFileSync(
      path.join(runDir, 'turn-layout-report.json'),
      JSON.stringify(bundle.compileReport, null, 2),
    );
  }
  if (bundle.exportMd) {
    fs.writeFileSync(path.join(runDir, 'export.md'), bundle.exportMd);
  }
  if (bundle.exportJson) {
    fs.writeFileSync(path.join(runDir, 'export.json'), JSON.stringify(bundle.exportJson, null, 2));
  }
  if (testInfo.error?.message) {
    fs.writeFileSync(path.join(runDir, 'playwright-error.txt'), testInfo.error.message);
  }

  console.log(`[real-llm] artifacts written: ${runDir}`);
  return runDir;
}

/** spec 内 `let artifactState: RealLlmArtifactState` + afterEach 挂载。 */
export type RealLlmArtifactState = {
  sessionId: string;
  exportMd?: string;
  exportJson?: unknown;
  compileReport?: TurnLayoutCompileReport;
  workspaceHints?: WorkspaceHints;
  errorMessage?: string;
};

export function createRealLlmArtifactHooks(sessionId: string) {
  const state: RealLlmArtifactState = { sessionId };
  return {
    state,
    afterEach: async (request: APIRequestContext, testInfo: TestInfo) => {
      await writeRealLlmArtifacts(testInfo, request, {
        sessionId: state.sessionId,
        exportMd: state.exportMd,
        exportJson: state.exportJson,
        compileReport: state.compileReport,
        errorMessage: state.errorMessage,
      }, state.workspaceHints);
    },
  };
}

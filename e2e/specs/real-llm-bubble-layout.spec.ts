/**
 * 真实 LLM E2E 测试：流式完成后消息结构正确性
 *
 * 复现 Bug：流式结束时的本地块布局会在 hydration 后被同 revision 的服务端
 * 原始消息重新拆分，导致重载前后 stored messages 结构不一致。
 *
 * 验证：流完成后立即从 API 拉取会话消息，检查结构是否交替正确。
 *
 * 前置条件：
 *   1. `cargo run -- serve` 运行
 *   2. 已配置 API_KEY
 *
 * 运行方式：
 *   cd e2e && npx playwright test specs/real-llm-bubble-layout.spec.ts
 */

import { test, expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { setupRealLLMSession, sendMessage } from "../fixtures/helpers";

function readApiKeyFromToml(filePath: string): string {
  try {
    const raw = fs.readFileSync(filePath, "utf8");
    const inAgentSection: string[] = [];
    let inAgent = false;
    for (const line of raw.split("\n")) {
      const trimmed = line.trim();
      if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
        const section = trimmed.slice(1, -1).trim();
        inAgent = section === "agent";
        continue;
      }
      if (inAgent && trimmed.startsWith("api_key")) {
        const eqIdx = trimmed.indexOf("=");
        if (eqIdx !== -1) {
          let val = trimmed.slice(eqIdx + 1).trim();
          if (
            (val.startsWith('"') && val.endsWith('"')) ||
            (val.startsWith("'") && val.endsWith("'"))
          ) {
            val = val.slice(1, -1);
          }
          if (val) inAgentSection.push(val);
        }
      }
    }
    if (inAgentSection.length > 0)
      return inAgentSection[inAgentSection.length - 1];
  } catch {
    /* ignore */
  }
  return "";
}

function resolveApiKey(): string {
  const env = process.env.API_KEY;
  if (env && env.trim()) return env.trim();
  const projectRoot = path.resolve(process.cwd(), "..");
  const fromConfig = readApiKeyFromToml(path.join(projectRoot, "config.toml"));
  if (fromConfig) return fromConfig;
  const fromDemo = readApiKeyFromToml(
    path.join(projectRoot, ".agent_demo.toml"),
  );
  if (fromDemo) return fromDemo;
  const dataHome =
    process.env.XDG_DATA_HOME ||
    path.join(process.env.HOME || "", ".local", "share");
  try {
    return fs
      .readFileSync(
        path.join(dataHome, "crabmate", "secrets", "client_llm"),
        "utf8",
      )
      .trim();
  } catch {
    return "";
  }
}

const API_KEY = resolveApiKey();
const SID = "s_e2e_msg_structure_" + Date.now();

interface StoredMessage {
  role: string;
  text: string;
  is_tool?: boolean;
  tool_call_id?: string;
  tool_name?: string;
}

interface StoredSessionSnapshot {
  messages: StoredMessage[];
  serverRevision?: number;
}

/** 从后端拉取会话消息及持久化 revision。 */
async function fetchSessionSnapshot(
  page: any,
  sid: string,
): Promise<StoredSessionSnapshot> {
  return page.evaluate(
    (s: string) =>
      fetch("/user-data/workspaces/current/sessions")
        .then((r: Response) => r.json())
        .then((d: any) => {
          const list = d.current?.sessions || d.sessions || [];
          const session = Array.isArray(list)
            ? list.find((x: any) => x.id === s)
            : null;
          return {
            messages: session ? session.messages || [] : [],
            serverRevision: session?.server_revision,
          };
        }),
    sid,
  );
}

function messageShapeSignature(messages: StoredMessage[]): string {
  return JSON.stringify(
    messages.map((m) => ({
      role: m.role,
      is_tool: Boolean(m.is_tool),
      text: m.text,
    })),
  );
}

async function waitForStableSessionMessages(
  page: any,
  sid: string,
  minimumCount: number,
): Promise<StoredMessage[]> {
  let messages: StoredMessage[] = [];
  let previousSignature = "";
  let stableReads = 0;
  for (let i = 0; i < 30; i++) {
    const snapshot = await fetchSessionSnapshot(page, sid);
    messages = snapshot.messages;
    const signature = JSON.stringify({
      revision: snapshot.serverRevision,
      messages: messageShapeSignature(messages),
    });
    const revisionReady = Number.isFinite(snapshot.serverRevision);
    if (
      revisionReady &&
      messages.length >= minimumCount &&
      signature === previousSignature
    ) {
      stableReads++;
      if (stableReads >= 2) return messages;
    } else {
      stableReads = 0;
    }
    previousSignature = signature;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(
    `会话消息在超时前未稳定：sid=${sid}, count=${messages.length}`,
  );
}

function analyzeMessages(messages: StoredMessage[]) {
  const assistantIndices: number[] = [];
  const toolIndices: number[] = [];
  messages.forEach((m, i) => {
    if (m.role === "assistant" && !m.is_tool) assistantIndices.push(i);
    else if (m.role === "tool" || m.is_tool) toolIndices.push(i);
  });
  const emptyAssistantMessages = assistantIndices.filter(
    (i) => !(messages[i].text || "").trim(),
  );
  let maxToolsBetweenAssistants = 0;
  for (let i = 0; i < assistantIndices.length - 1; i++) {
    const start = assistantIndices[i];
    const end = assistantIndices[i + 1];
    const toolsBetween = messages
      .slice(start + 1, end)
      .filter((m) => m.role === "tool" || m.is_tool).length;
    maxToolsBetweenAssistants = Math.max(
      maxToolsBetweenAssistants,
      toolsBetween,
    );
  }
  return {
    assistantIndices,
    toolIndices,
    emptyAssistantMessages,
    maxToolsBetweenAssistants,
  };
}

async function waitForReadyWhileApproving(page: any, timeoutMs: number) {
  const deadline = Date.now() + timeoutMs;
  const statusBar = page.locator('[data-testid="status-bar"]');
  const approvalModal = page.locator('[data-testid="approval-modal"]');
  while (Date.now() < deadline) {
    if (await approvalModal.isVisible()) {
      await page.locator('[data-testid="approval-allow-always"]').click();
      await expect(approvalModal).not.toBeVisible({ timeout: 10_000 });
      continue;
    }
    if ((await statusBar.textContent())?.includes("就绪")) return;
    await page.waitForTimeout(250);
  }
  throw new Error(`流在 ${timeoutMs}ms 内未进入就绪状态`);
}

test.describe("真实 LLM：流式后消息结构", () => {
  const runTest = API_KEY ? test : test.skip;

  runTest("流式布局在重载前后保持一致且无空气泡", async ({ page }) => {
    // 先创建临时工作区目录（在项目根目录下，确保在 workspace_allowed_roots 范围内）
    const wsDir = path.resolve(process.cwd(), "..", ".e2e_tmp_" + Date.now());
    fs.mkdirSync(wsDir, { recursive: true });

    // 再设置会话和工作区（顺序重要：先设工作区再设会话，避免工作区变更后会话丢失）
    await setupRealLLMSession(page, SID, API_KEY);
    await page.evaluate((dir: string) => {
      return fetch("/workspace", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: dir }),
      });
    }, wsDir);
    await page.reload({ waitUntil: "networkidle", timeout: 20000 });
    await page.waitForSelector('[data-testid="chat-composer-input"]', {
      timeout: 15000,
    });
    // 重新创建空会话（工作区变更后旧会话不在新工作区中）
    await page.evaluate((s: string) => {
      const body = JSON.stringify({
        sessions: [
          {
            id: s,
            title: "e2e-bubble",
            draft: "",
            messages: [],
            updated_at: Date.now(),
            pinned: false,
            starred: false,
          },
        ],
        active_session_id: s,
      });
      return fetch("/user-data/workspaces/current/sessions", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body,
      });
    }, SID);
    await page.reload({ waitUntil: "networkidle", timeout: 20000 });
    await page.waitForSelector('[data-testid="chat-composer-input"]', {
      timeout: 15000,
    });

    await sendMessage(page, "编写一个简单c++程序，使用cmake编译执行");

    await waitForReadyWhileApproving(page, 180_000);

    // 等待防抖持久化和异步 hydration 均稳定，避免比较中间快照。
    let messages = await waitForStableSessionMessages(page, SID, 3);

    // 基础校验：至少有若干条消息
    expect(messages.length).toBeGreaterThanOrEqual(3);

    // 统计 DOM 中实际渲染的非工具气泡数
    const renderedBubbleCount = await page.evaluate(
      () =>
        document.querySelectorAll('[data-testid="chat-message-row"]').length,
    );

    let analysis = analyzeMessages(messages);

    // ── 终端输出调试信息（必须在断言之前，断言失败后不会执行到此处）──
    console.log(`消息总数: ${messages.length}`);
    console.log(`DOM 气泡数 (chat-message-row): ${renderedBubbleCount}`);
    console.log(`助手消息数: ${analysis.assistantIndices.length}`);
    console.log(`工具消息数: ${analysis.toolIndices.length}`);
    console.log(`助手间最大连续工具数: ${analysis.maxToolsBetweenAssistants}`);
    console.log(`空助手消息数: ${analysis.emptyAssistantMessages.length}`);
    messages.forEach((m, i) => {
      const isAssistant = m.role === "assistant" && !m.is_tool;
      const maxLen = isAssistant ? undefined : 50;
      const preview = (m.text || "").trim().slice(0, maxLen);
      console.log(
        `  [${i}] role=${m.role} is_tool=${m.is_tool} text="${preview}"` +
          (isAssistant && !(m.text || "").trim() ? " ← EMPTY" : ""),
      );
    });

    // ── 会话一致性断言 ──
    // 核心 Bug（135832.md vs 135859.md）：流式结束后的 stored_messages 与重载后不一致，
    // 表现为助手正文在多条消息间合并/移动。
    // 此处：保存流式结束时的消息 → 重载页面 → 重新拉取 → 逐字段对比。
    const messagesBeforeReload = JSON.parse(JSON.stringify(messages)); // deep clone
    await page.reload({ waitUntil: "networkidle", timeout: 20000 });
    await page.waitForSelector('[data-testid="chat-composer-input"]', {
      timeout: 15000,
    });
    const messagesAfter = await waitForStableSessionMessages(
      page,
      SID,
      messagesBeforeReload.length,
    );
    // 比较每条消息的 role、is_tool、text（忽略 id / created_at 等易变字段）
    expect(messagesAfter.length).toBe(messagesBeforeReload.length);
    for (let i = 0; i < messagesBeforeReload.length; i++) {
      const a = messagesBeforeReload[i];
      const b = messagesAfter[i];
      // 若 text 不一致，记录详细差异
      if (a.role !== b.role || a.is_tool !== b.is_tool || a.text !== b.text) {
        console.log(
          `[${i}] 差异: role=${a.role}→${b.role} is_tool=${a.is_tool}→${b.is_tool}`,
        );
        console.log(`  重载前 text="${(a.text || "").trim().slice(0, 200)}"`);
        console.log(`  重载后 text="${(b.text || "").trim().slice(0, 200)}"`);
      }
      expect(a.role).toBe(b.role);
      expect(a.is_tool).toBe(b.is_tool);
      expect(a.text).toBe(b.text);
    }
    // 更新 messages 引用为重载后的数据，供后续断言使用
    messages = messagesAfter;
    analysis = analyzeMessages(messagesAfter);

    // ── 核心断言 ──
    // 注意：LLM 可能将所有正文输出在单次响应中（仅产生 2 条助手消息：意图 + 正文），
    // 也可能多轮交替（意图 + 预工具说明 + 工具后说明 + 终答 → 4+ 条）。
    // 以下断言检测前端存储/渲染的严重结构问题，而非 LLM 输出模式。

    // 至少 2 条独立的非工具助手消息（意图分析 + 至少 1 轮正文）
    // Bug（极端情况）：所有文本合并为 1 条（只有 1 条非工具助手）
    expect(analysis.assistantIndices.length).toBeGreaterThanOrEqual(2);
    expect(analysis.toolIndices.length).toBeGreaterThan(0);

    // 没有空的助手消息
    // Bug：旋转后无 delta 跟进会产生空气泡（如 [N] role=assistant text=""）
    expect(analysis.emptyAssistantMessages).toEqual([]);

    // DOM 中至少 3 个气泡行（用户 + 至少 2 个助手气泡）
    // 单轮正文：用户 + 意图 + 正文 = 3 行
    // 多轮交替：用户 + N 个助手 = 3+ 行
    expect(renderedBubbleCount).toBeGreaterThanOrEqual(3);

    // 不应出现错误提示
    const errorToasts = await page
      .locator('[data-testid="error-toast"]')
      .count();
    expect(errorToasts).toBe(0);
  });
});

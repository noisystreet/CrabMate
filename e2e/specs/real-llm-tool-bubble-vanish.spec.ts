/**
 * 真实 LLM：按 `chat_export_20260730_225530.md` 同类任务复现
 * 「有工具调用时助手气泡先出现再消失」。
 *
 * 提示词对齐导出：「分析一下当前目录下的源码」；工作区含迷你 `cpp-demo/`。
 *
 * 流中监控（正文助手首次出现后）：
 *   1. 正文助手 section 不得归零
 *   2. 已见正文不得整段从 transcript 消失（允许换 section id / 移交 commentary）
 *   3. 可见性不得 true→false→true 闪回
 *
 * 前置：
 *   1. `cargo run -- serve` 在 127.0.0.1:8080
 *   2. `API_KEY` **或** 本机系统钥匙串已有 `client_llm`（测试进程不打印/导出明文）
 *
 * 运行：
 *   cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/real-llm-tool-bubble-vanish.spec.ts
 */
import { expect, test, type Page } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import {
  sendMessage,
  setupRealLLMSession,
  waitForHealth,
} from "../fixtures/helpers";

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
    if (inAgentSection.length > 0) {
      return inAgentSection[inAgentSection.length - 1];
    }
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
  return "";
}

const API_KEY = resolveApiKey();
const PROMPT = "分析一下当前目录下的源码";

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
  toolCardSeen: boolean;
  /** 首次归零瞬间 / 归零后恢复瞬间的 transcript 结构，用于定位旁白落到哪一行。 */
  zeroSnapshot: string[] | null;
  recoverSnapshot: string[] | null;
};

async function secretsClientLlmSet(page: Page): Promise<boolean> {
  return page.evaluate(async () => {
    const response = await fetch("/user-data/secrets/status");
    if (!response.ok) return false;
    const data = (await response.json()) as {
      client_llm?: { set?: boolean };
    };
    return Boolean(data.client_llm?.set);
  });
}

/** 无 API_KEY 时不写 secrets（避免清空钥匙串），只建会话与 LLM 覆盖。 */
async function setupSessionPreferringKeyring(
  page: Page,
  sid: string,
  apiKey: string,
) {
  if (apiKey) {
    await setupRealLLMSession(page, sid, apiKey);
    return;
  }
  await page.goto("/", { waitUntil: "networkidle", timeout: 20_000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15_000,
  });
  await page.evaluate(() =>
    fetch("/user-data/llm-overrides", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        client_llm: {
          api_base: "https://api.deepseek.com",
          model: "deepseek-v4-flash",
          llm_context_tokens: "1000000",
          llm_thinking_mode: "off",
        },
      }),
    }),
  );
  await page.evaluate(() =>
    fetch("/user-data/prefs", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        locale: "zh",
        theme: "light",
        side_panel_view: "hidden",
        side_width: 280,
        editor_layout_mode: false,
      }),
    }).catch(() => {}),
  );
  await page.evaluate((s: string) => {
    const body = JSON.stringify({
      sessions: [
        {
          id: s,
          title: "e2e-real-vanish",
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
    }).catch(() => {});
  }, sid);
  await page.reload({ waitUntil: "networkidle", timeout: 20_000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15_000,
  });
}

function seedMiniCppWorkspace(): string {
  const projectRoot = path.resolve(process.cwd(), "..");
  const wsDir = path.join(projectRoot, `.e2e_tmp_vanish_${Date.now()}`);
  fs.mkdirSync(wsDir, { recursive: true });
  const demo = path.join(wsDir, "cpp-demo");
  fs.mkdirSync(demo, { recursive: true });
  fs.writeFileSync(
    path.join(demo, "main.cpp"),
    `#include <iostream>
#include <vector>
#include <numeric>
int main() {
  std::vector<int> fib{0, 1};
  for (int i = 2; i < 15; ++i) fib.push_back(fib[i - 1] + fib[i - 2]);
  for (int v : fib) std::cout << v << " ";
  std::cout << "\\nsum=" << std::accumulate(fib.begin(), fib.end(), 0) << "\\n";
  return 0;
}
`,
  );
  fs.writeFileSync(
    path.join(demo, "CMakeLists.txt"),
    `cmake_minimum_required(VERSION 3.10)
project(CppDemo VERSION 1.0.0 LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_executable(demo main.cpp)
`,
  );
  fs.writeFileSync(
    path.join(wsDir, "package.json"),
    `{ "dependencies": { "@playwright/test": "^1.62.0" } }\n`,
  );
  return wsDir;
}

async function bindWorkspaceAndSession(page: Page, wsDir: string, sid: string) {
  await page.evaluate((dir: string) => {
    return fetch("/workspace", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: dir }),
    });
  }, wsDir);
  await page.reload({ waitUntil: "networkidle", timeout: 20_000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15_000,
  });
  await page.evaluate((s: string) => {
    const body = JSON.stringify({
      sessions: [
        {
          id: s,
          title: "e2e-real-vanish",
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
  }, sid);
  await page.reload({ waitUntil: "networkidle", timeout: 20_000 });
  await page.waitForSelector('[data-testid="chat-composer-input"]', {
    timeout: 15_000,
  });
}

async function waitForReadyWhileApproving(page: Page, timeoutMs: number) {
  const deadline = Date.now() + timeoutMs;
  const statusBar = page.locator('[data-testid="status-bar"]');
  const approvalModal = page.locator('[data-testid="approval-modal"]');
  while (Date.now() < deadline) {
    if (await approvalModal.isVisible().catch(() => false)) {
      const allowAlways = page.locator('[data-testid="approval-allow-always"]');
      if (await allowAlways.isVisible().catch(() => false)) {
        await allowAlways.click();
      } else {
        await page.locator('[data-testid="approval-allow-once"]').click();
      }
      await expect(approvalModal).not.toBeVisible({ timeout: 10_000 });
      continue;
    }
    if ((await statusBar.textContent())?.includes("就绪")) return;
    await page.waitForTimeout(250);
  }
  throw new Error(`流在 ${timeoutMs}ms 内未进入就绪状态`);
}

test.describe("真实 LLM：工具回合气泡闪没", () => {
  test("源码分析多工具流：助手气泡不得出现后消失", async ({ page }) => {
    const baseUrl = `http://127.0.0.1:${process.env.CRABMATE_PORT || "8080"}`;
    await waitForHealth(baseUrl);

    const sid = `s_e2e_real_vanish_${Date.now()}`;
    await setupSessionPreferringKeyring(page, sid, API_KEY);

    if (!API_KEY) {
      const keyringOk = await secretsClientLlmSet(page);
      if (!keyringOk) {
        test.skip(
          true,
          "未设置 API_KEY 且系统钥匙串无 client_llm，跳过真实 LLM 用例",
        );
        return;
      }
    }

    const wsDir = seedMiniCppWorkspace();
    try {
      await bindWorkspaceAndSession(page, wsDir, sid);
      await expect(page.getByTestId("chat-tui-stream-view")).toBeVisible();

      await page.evaluate((sampleMs) => {
        const state = globalThis as typeof globalThis & {
          __cmRealVanishStop?: () => MonitorResult;
        };
        const startedAt = performance.now();
        const gaps: VanishGap[] = [];
        let samples = 0;
        let firstBodyAtMs: number | null = null;
        let maxBodyCount = 0;
        let toolCardSeen = false;
        /** 首次正文助手的稳定前缀（定长）；之后只要求 transcript 仍含该子串。 */
        let frozenPrefix: string | null = null;
        let prefixGonePendingFlicker = false;
        let lastZeroGapAt = -1_000;
        let lastMissingGapAt = -1_000;
        let zeroSnapshot: string[] | null = null;
        let recoverSnapshot: string[] | null = null;
        let sawZero = false;

        const nowMs = () => Math.round(performance.now() - startedAt);

        const snapshotSections = (tMs: number): string[] => {
          const root = document.querySelector(
            '[data-testid="chat-tui-transcript"]',
          );
          const rows = [
            ...(root?.querySelectorAll<HTMLElement>("section") ?? []),
          ];
          return [
            `t=${tMs} sections=${rows.length}`,
            ...rows.map((el) => {
              const text = (el.innerText ?? "").replace(/\s+/g, " ").trim();
              return `${el.className} | len=${text.length} | ${text.slice(0, 60)}`;
            }),
          ];
        };

        const sample = () => {
          samples += 1;
          const tMs = nowMs();
          if (
            document.querySelector('[data-testid="chat-tool-card"]') ||
            document.querySelector('[data-testid="chat-tui-tool-process"]') ||
            document.querySelector("section.chat-tui-turn--tool")
          ) {
            toolCardSeen = true;
          }
          const assistants = [
            ...document.querySelectorAll<HTMLElement>(
              "section.chat-tui-turn--assistant",
            ),
          ];
          const bodyAssistants = assistants.filter((el) => {
            const text = (el.innerText ?? "").replace(/\s+/g, " ").trim();
            if (!text) return false;
            if (text.includes("意图分析")) return false;
            return true;
          });
          maxBodyCount = Math.max(maxBodyCount, bodyAssistants.length);
          const transcriptText = (
            document.querySelector('[data-testid="chat-tui-transcript"]')
              ?.textContent ?? ""
          ).replace(/\s+/g, " ");

          if (bodyAssistants.length > 0 && frozenPrefix === null) {
            const first = (bodyAssistants[0].innerText ?? "")
              .replace(/\s+/g, " ")
              .trim();
            if (first.length >= 12) {
              frozenPrefix = first.slice(0, 18);
              firstBodyAtMs = tMs;
            }
          }

          if (firstBodyAtMs === null) return;

          if (bodyAssistants.length === 0) {
            if (zeroSnapshot === null) {
              zeroSnapshot = snapshotSections(tMs);
            }
            sawZero = true;
            // 节流：同一闪空窗口只记一次
            if (tMs - lastZeroGapAt > 40) {
              gaps.push({
                reason: "assistant_body_sections_zero",
                needle: frozenPrefix ?? "*",
                tMs,
                bodyAssistantCount: 0,
              });
              lastZeroGapAt = tMs;
            }
          } else if (sawZero && recoverSnapshot === null) {
            recoverSnapshot = snapshotSections(tMs);
          }

          if (frozenPrefix) {
            const visible = transcriptText.includes(frozenPrefix);
            if (visible) {
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
        state.__cmRealVanishStop = () => {
          mo.disconnect();
          window.clearInterval(timer);
          return {
            gaps,
            samples,
            firstBodyAtMs,
            maxBodyCount,
            toolCardSeen,
            zeroSnapshot,
            recoverSnapshot,
          };
        };
      }, 8);

      await sendMessage(page, PROMPT);
      await waitForReadyWhileApproving(page, 240_000);

      await expect(page.locator('[data-testid="status-bar"]')).toContainText(
        "就绪",
        { timeout: 5_000 },
      );
      await page.waitForTimeout(800);

      const result = await page.evaluate(() => {
        const state = globalThis as typeof globalThis & {
          __cmRealVanishStop?: () => MonitorResult;
        };
        return state.__cmRealVanishStop?.() ?? null;
      });

      expect(result, "vanish monitor must be installed").not.toBeNull();
      expect(
        result!.firstBodyAtMs,
        "at least one assistant body bubble must appear",
      ).not.toBeNull();
      expect(
        result!.toolCardSeen || result!.maxBodyCount >= 1,
        "expected tool cards or multi-step assistant bodies (tool-call path)",
      ).toBeTruthy();

      // 压缩同 reason 的连续采样（节流后仍可能有多段闪空窗口）
      const compactGaps = result!.gaps.filter((gap, index, all) => {
        if (index === 0) return true;
        const prev = all[index - 1];
        return !(
          prev.reason === gap.reason &&
          prev.needle === gap.needle &&
          gap.tMs - prev.tMs < 120
        );
      });

      const zeroGaps = compactGaps.filter(
        (g) => g.reason === "assistant_body_sections_zero",
      );
      expect(
        zeroGaps,
        `real-LLM assistant body bubble count hit zero after first paint: ${JSON.stringify(
          {
            zeroGaps,
            allGaps: compactGaps.slice(0, 20),
            samples: result!.samples,
            firstBodyAtMs: result!.firstBodyAtMs,
            maxBodyCount: result!.maxBodyCount,
            toolCardSeen: result!.toolCardSeen,
            zeroSnapshot: result!.zeroSnapshot,
            recoverSnapshot: result!.recoverSnapshot,
          },
          null,
          2,
        )}`,
      ).toEqual([]);

      expect(
        compactGaps,
        `real-LLM bubble vanished during tool stream: ${JSON.stringify({
          gaps: compactGaps.slice(0, 20),
          gapCount: compactGaps.length,
          samples: result!.samples,
          firstBodyAtMs: result!.firstBodyAtMs,
          maxBodyCount: result!.maxBodyCount,
          toolCardSeen: result!.toolCardSeen,
        })}`,
      ).toEqual([]);
      expect(result!.samples).toBeGreaterThan(50);
    } finally {
      fs.rmSync(wsDir, { recursive: true, force: true });
    }
  });
});

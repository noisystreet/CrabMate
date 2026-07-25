import { expect, Page, Route, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

type PersistedMessage = {
  id: string;
  role: string;
  text?: string;
  is_tool?: boolean;
};

type PersistedSession = {
  id: string;
  layout_schema_version?: number;
  server_revision?: number;
  messages?: PersistedMessage[];
};

function toolTurnSse(
  toolCallId: string,
  commentary: string,
  answer: string,
): string {
  return [
    'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
    `id: 2\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-before-${toolCallId}","kind":"commentary","beforeToolCallId":"${toolCallId}"}}\n\n`,
    `id: 3\ndata: ${commentary}\n\n`,
    `id: 4\ndata: {"type":"CUSTOM","customType":"turn_segment_end","data":{"segmentId":"seg-before-${toolCallId}"}}\n\n`,
    `id: 5\ndata: {"type":"TOOL_CALL_START","toolCallId":"${toolCallId}","name":"read_file","summary":"读取文件"}\n\n`,
    `id: 6\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"${toolCallId}","content":"ok","metadata":{"name":"read_file","ok":true,"summary":"读取成功"}}\n\n`,
    'id: 7\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
    'id: 8\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
    `id: 9\ndata: ${answer}\n\n`,
    'id: 10\ndata: {"type":"RUN_FINISHED"}\n\n',
  ].join("");
}

function answerOnlySse(answer: string): string {
  return [
    'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
    `id: 2\ndata: ${answer}\n\n`,
    'id: 3\ndata: {"type":"RUN_FINISHED"}\n\n',
  ].join("");
}

async function installSequentialStreams(page: Page, streams: string[]) {
  let callIndex = 0;
  await page.route("**/chat/stream", (route: Route) => {
    if (route.request().method() !== "POST") {
      return route.continue();
    }
    const body = streams[Math.min(callIndex, streams.length - 1)];
    callIndex += 1;
    return route.fulfill({
      status: 200,
      headers: {
        "content-type": "text/event-stream; charset=utf-8",
        "x-conversation-id": "e2e-v2-multi-turn",
        "x-stream-job-id": String(callIndex),
      },
      body,
    });
  });
}

async function persistedSession(page: Page, sid: string) {
  return page.evaluate(async (sessionId) => {
    const response = await fetch("/user-data/workspaces/current/sessions");
    const data = await response.json();
    return (
      (data.sessions as PersistedSession[] | undefined)?.find(
        (session) => session.id === sessionId,
      ) ?? null
    );
  }, sid);
}

async function waitForAnswer(page: Page, answer: string) {
  await expect(
    page.locator('[data-testid="chat-messages-scroller"]'),
  ).toContainText(answer, { timeout: 15_000 });
  await expect(page.locator('[data-testid="status-bar"]')).toContainText(
    "就绪",
    { timeout: 15_000 },
  );
}

test.describe("v2 多回合边界", () => {
  test("不同回合复用 tool_call_id 时 commentary 行仍独立", async ({ page }) => {
    const firstCommentary = "第一轮准备读取 alpha。";
    const secondCommentary = "第二轮准备读取 beta。";
    const firstAnswer = "第一轮读取完成。";
    const secondAnswer = "第二轮读取完成。";
    await installSequentialStreams(page, [
      toolTurnSse("tc-reused", firstCommentary, firstAnswer),
      toolTurnSse("tc-reused", secondCommentary, secondAnswer),
    ]);
    const sid = `s_e2e_reused_tool_id_${Date.now()}`;
    await seedSession(page, sid);

    await sendMessage(page, "读取 alpha");
    await waitForAnswer(page, firstAnswer);
    await sendMessage(page, "读取 beta");
    await waitForAnswer(page, secondAnswer);

    await expect
      .poll(async () => {
        const session = await persistedSession(page, sid);
        return (session?.messages ?? [])
          .filter(
            (message) =>
              message.text?.includes(firstCommentary) ||
              message.text?.includes(secondCommentary),
          )
          .map((message) => ({ id: message.id, text: message.text }));
      })
      .toEqual([
        expect.objectContaining({ text: firstCommentary }),
        expect.objectContaining({ text: secondCommentary }),
      ]);

    const session = await persistedSession(page, sid);
    const commentaryRows = (session?.messages ?? []).filter(
      (message) =>
        message.text?.includes(firstCommentary) ||
        message.text?.includes(secondCommentary),
    );
    expect(new Set(commentaryRows.map((message) => message.id)).size).toBe(2);
  });

  test("较新服务端 revision 的新增回合不会被本地 v2 投影遮蔽", async ({
    page,
  }) => {
    const sid = `s_e2e_newer_revision_${Date.now()}`;
    const conversationId = `conv-newer-${Date.now()}`;
    const serverOnlyAnswer = "这是另一浏览器新增的服务端回答。";
    await seedSession(page, sid);
    await page.route("**/conversation/messages?**", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          conversation_id: conversationId,
          revision: 2,
          messages: [
            { role: "user", content: "本地已有问题" },
            { role: "assistant", content: "本地已有回答" },
            { role: "user", content: "另一浏览器新增问题" },
            { role: "assistant", content: serverOnlyAnswer },
          ],
          total_count: 4,
          window_start_index: 0,
          has_older: false,
        }),
      }),
    );
    await page.evaluate(
      async ({ sessionId, cid }) => {
        await fetch("/user-data/workspaces/current/sessions", {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            sessions: [
              {
                id: sessionId,
                layout_schema_version: 2,
                title: "e2e-newer-revision",
                draft: "",
                updated_at: Date.now(),
                pinned: false,
                starred: false,
                server_conversation_id: cid,
                server_revision: 1,
                messages: [
                  {
                    id: "u-local",
                    role: "user",
                    text: "本地已有问题",
                    reasoning_text: "",
                    created_at: 1,
                  },
                  {
                    id: "turn-commentary-local",
                    role: "assistant",
                    text: "本地 v2 finalized 行。",
                    reasoning_text: "",
                    created_at: 2,
                  },
                ],
              },
            ],
            active_session_id: sessionId,
          }),
        });
      },
      { sessionId: sid, cid: conversationId },
    );

    await page.reload({ waitUntil: "networkidle", timeout: 20_000 });
    await page.waitForSelector('[data-testid="chat-composer-input"]');

    await expect(
      page.locator('[data-testid="chat-messages-scroller"]'),
    ).toContainText(serverOnlyAnswer, { timeout: 15_000 });
    await expect
      .poll(async () => {
        const session = await persistedSession(page, sid);
        return {
          revision: session?.server_revision,
          hasServerAnswer: (session?.messages ?? []).some((message) =>
            message.text?.includes(serverOnlyAnswer),
          ),
        };
      })
      .toEqual({ revision: 2, hasServerAnswer: true });
  });

  test("工具回合后的无工具终答不会被旧 commentary 清理", async ({ page }) => {
    const commentary = "第一轮先读取配置。";
    const firstAnswer = "配置读取完成。";
    const secondAnswer = "第二轮直接回答且不调用工具。";
    await installSequentialStreams(page, [
      toolTurnSse("tc-config", commentary, firstAnswer),
      answerOnlySse(secondAnswer),
    ]);
    const sid = `s_e2e_tool_then_answer_${Date.now()}`;
    await seedSession(page, sid);

    await sendMessage(page, "读取配置");
    await waitForAnswer(page, firstAnswer);
    await sendMessage(page, "直接总结");
    await waitForAnswer(page, secondAnswer);
    await page.reload({ waitUntil: "networkidle", timeout: 20_000 });
    await page.waitForSelector('[data-testid="chat-composer-input"]');

    const scroller = page.locator('[data-testid="chat-messages-scroller"]');
    await expect(scroller).toContainText(commentary);
    await expect(scroller).toContainText(firstAnswer);
    await expect(scroller).toContainText(secondAnswer);
  });
});

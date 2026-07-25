import { expect, test } from "@playwright/test";
import { seedSession, sendMessage } from "../fixtures/helpers";

const DELAYED_SSE_CHUNKS = [
  'id: 1\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
  "id: 2\ndata: 我来先了解一下\n\n",
  "id: 3\ndata: 工作区的内容，\n\n",
  "id: 4\ndata: 然后创建 C++ 程序。\n\n",
  'id: 5\ndata: {"type":"TOOL_CALL_START","toolCallId":"t1","name":"list_tree","summary":"列出目录"}\n\n',
  'id: 6\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"t1","content":"empty","metadata":{"name":"list_tree","ok":true,"summary":"列出目录"}}\n\n',
  'id: 7\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
  'id: 8\ndata: {"type":"CUSTOM","customType":"turn_segment_end","data":{"segmentId":"seg-1"}}\n\n',
  'id: 9\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-2","kind":"answer"}}\n\n',
  'id: 10\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
  "id: 11\ndata: 工作区是空的，\n\n",
  "id: 12\ndata: 现在创建源文件。\n\n",
  'id: 13\ndata: {"type":"TOOL_CALL_START","toolCallId":"t2","name":"create_file","summary":"创建 hello.cpp"}\n\n',
  'id: 14\ndata: {"type":"TOOL_CALL_RESULT","toolCallId":"t2","content":"hello.cpp","metadata":{"name":"create_file","ok":true,"summary":"创建 hello.cpp"}}\n\n',
  'id: 15\ndata: {"type":"CUSTOM","customType":"turn_tool_phase_end","data":{"phase":"tool_end"}}\n\n',
  'id: 16\ndata: {"type":"CUSTOM","customType":"turn_segment_end","data":{"segmentId":"seg-2"}}\n\n',
  'id: 17\ndata: {"type":"CUSTOM","customType":"turn_segment_start","data":{"segmentId":"seg-3","kind":"answer"}}\n\n',
  'id: 18\ndata: {"type":"CUSTOM","customType":"assistant_answer_phase"}\n\n',
  "id: 19\ndata: 完成。C++ 程序已成功\n\n",
  "id: 20\ndata: 编译并执行。\n\n",
  'id: 21\ndata: {"type":"RUN_FINISHED","threadId":"","runId":"1"}\n\n',
  'id: 22\ndata: {"type":"CUSTOM","customType":"conversation_saved","data":{"revision":1}}\n\n',
];

type Overlap = {
  first: { id: string; top: number; bottom: number };
  second: { id: string; top: number; bottom: number };
  verticalOverlap: number;
};

type StreamSnapshot = {
  chunkIndex: number;
  rowIds: string[];
  rowTexts: string[];
  toolCount: number;
};

test("delayed multi-tool stream reuses each active row without overlap", async ({
  page,
}) => {
  await page.addInitScript((chunks: string[]) => {
    Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
      configurable: true,
      value: { invoke: () => Promise.resolve(null) },
    });

    const state = globalThis as typeof globalThis & {
      __cmFirstMessageOverlap?: Overlap | null;
      __cmOverlapTimer?: number;
      __cmChunkIndex?: number;
      __cmStreamSnapshots?: StreamSnapshot[];
    };
    state.__cmFirstMessageOverlap = null;
    state.__cmChunkIndex = -1;
    state.__cmStreamSnapshots = [];

    const inspect = () => {
      const rows = [
        ...document.querySelectorAll<HTMLElement>(
          ".messages-inner .msg-with-select",
        ),
      ]
        .filter((element) => element.offsetParent !== null)
        .map((element) => {
          const rect = element.getBoundingClientRect();
          return {
            id:
              element.querySelector<HTMLElement>(".msg")?.id ||
              element.textContent?.slice(0, 32) ||
              "",
            text: element.textContent ?? "",
            top: rect.top,
            bottom: rect.bottom,
            left: rect.left,
            right: rect.right,
          };
        })
        .sort((a, b) => a.top - b.top);
      state.__cmStreamSnapshots?.push({
        chunkIndex: state.__cmChunkIndex ?? -1,
        rowIds: rows.map((row) => row.id),
        rowTexts: rows.map((row) => row.text),
        toolCount: document.querySelectorAll('[data-testid="chat-tool-card"]')
          .length,
      });

      for (let firstIndex = 0; firstIndex < rows.length; firstIndex += 1) {
        for (
          let secondIndex = firstIndex + 1;
          secondIndex < rows.length;
          secondIndex += 1
        ) {
          const first = rows[firstIndex];
          const second = rows[secondIndex];
          if (second.top >= first.bottom - 0.5) break;
          const horizontalOverlap =
            Math.min(first.right, second.right) -
            Math.max(first.left, second.left);
          const verticalOverlap =
            Math.min(first.bottom, second.bottom) -
            Math.max(first.top, second.top);
          if (horizontalOverlap > 0.5 && verticalOverlap > 0.5) {
            state.__cmFirstMessageOverlap ??= {
              first,
              second,
              verticalOverlap,
            };
            return;
          }
        }
      }
    };

    state.__cmOverlapTimer = window.setInterval(inspect, 8);
    const originalFetch = window.fetch.bind(window);
    window.fetch = (input, init) => {
      const url =
        typeof input === "string"
          ? input
          : input instanceof URL
            ? input.href
            : input.url;
      const method =
        init?.method ?? (input instanceof Request ? input.method : "GET");
      if (url.includes("/chat/stream") && method === "POST") {
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
              window.setTimeout(push, 300);
            };
            push();
          },
        });
        return Promise.resolve(
          new Response(body, {
            status: 200,
            headers: {
              "content-type": "text/event-stream; charset=utf-8",
              "x-conversation-id": "e2e-overlap",
              "x-stream-job-id": "1",
            },
          }),
        );
      }
      return originalFetch(input, init);
    };
  }, DELAYED_SSE_CHUNKS);

  await seedSession(page, "s_e2e_stream_overlap");
  await sendMessage(page, "编写一个简单 C++ 程序");
  await expect(
    page.locator('[data-testid="chat-messages-scroller"]'),
  ).toContainText("C++ 程序已成功编译并执行", { timeout: 30_000 });
  await page.waitForTimeout(300);

  const overlap = await page.evaluate(() => {
    const state = globalThis as typeof globalThis & {
      __cmFirstMessageOverlap?: Overlap | null;
      __cmOverlapTimer?: number;
      __cmStreamSnapshots?: StreamSnapshot[];
    };
    if (state.__cmOverlapTimer !== undefined) {
      clearInterval(state.__cmOverlapTimer);
    }
    return {
      overlap: state.__cmFirstMessageOverlap ?? null,
      snapshots: state.__cmStreamSnapshots ?? [],
    };
  });

  expect(overlap.overlap).toBeNull();

  const lastSnapshotForChunk = new Map<number, StreamSnapshot>();
  for (const snapshot of overlap.snapshots) {
    lastSnapshotForChunk.set(snapshot.chunkIndex, snapshot);
  }
  for (const deltaGroup of [
    [1, 2, 3],
    [10, 11],
    [18, 19],
  ]) {
    const snapshots = deltaGroup.map((index) => {
      const snapshot = lastSnapshotForChunk.get(index);
      expect(snapshot, `missing snapshot for chunk ${index}`).toBeDefined();
      return snapshot!;
    });
    const expectedIds = snapshots[0].rowIds;
    for (const snapshot of snapshots.slice(1)) {
      expect(snapshot.rowIds).toEqual(expectedIds);
    }
  }
});

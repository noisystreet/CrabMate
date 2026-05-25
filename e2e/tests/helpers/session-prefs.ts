import { expect, type APIRequestContext } from '@playwright/test';

export type E2eSessionRow = {
  id: string;
  title: string;
  draft: string;
  messages: unknown[];
  updated_at: number;
  pinned: boolean;
  starred: boolean;
  server_conversation_id?: string;
  server_revision?: number;
};

export async function putWorkspaceSessions(
  request: APIRequestContext,
  sessions: E2eSessionRow[],
  activeSessionId: string,
): Promise<void> {
  const put = await request.put('/user-data/workspaces/current/sessions', {
    data: { sessions, active_session_id: activeSessionId },
  });
  expect(put.status()).toBe(204);
}

/** 将 active 会话绑定到服务端 conversation_id，便于刷新后水合。 */
export async function putActiveSessionWithServerConversation(
  request: APIRequestContext,
  sessionId: string,
  conversationId: string,
  opts?: { title?: string; serverRevision?: number },
): Promise<void> {
  await putWorkspaceSessions(
    request,
    [
      {
        id: sessionId,
        title: opts?.title ?? 'E2E session',
        draft: '',
        messages: [],
        updated_at: 1,
        pinned: false,
        starred: false,
        server_conversation_id: conversationId,
        server_revision: opts?.serverRevision ?? 1,
      },
    ],
    sessionId,
  );
}

export async function resetWorkspaceToDefault(request: APIRequestContext): Promise<void> {
  const res = await request.post('/workspace', { data: { path: null } });
  expect(res.ok()).toBeTruthy();
}

/** 重置为仅含一条空本地会话，避免其它 spec 留下的 active / server_conversation_id 污染 UI。 */
export async function putFreshLocalSession(
  request: APIRequestContext,
  sessionId: string,
  title = 'E2E smoke',
): Promise<void> {
  await putWorkspaceSessions(
    request,
    [
      {
        id: sessionId,
        title,
        draft: '',
        messages: [],
        updated_at: Date.now(),
        pinned: false,
        starred: false,
      },
    ],
    sessionId,
  );
}

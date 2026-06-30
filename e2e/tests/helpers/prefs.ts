import { expect, type APIRequestContext } from '@playwright/test';

export type PutPrefsPayload = {
  locale?: string;
  theme?: string;
  side_panel_view?: string;
  side_width?: number;
  timeline_panel_expanded?: boolean;
  editor_layout_mode?: boolean;
};

export async function putUserPrefs(request: APIRequestContext, prefs: PutPrefsPayload): Promise<void> {
  const put = await request.put('/user-data/prefs', {
    data: {
      locale: prefs.locale ?? 'zh',
      theme: prefs.theme ?? 'light',
      side_panel_view: prefs.side_panel_view ?? 'hidden',
      side_width: prefs.side_width ?? 280,
      ...(prefs.timeline_panel_expanded !== undefined
        ? { timeline_panel_expanded: prefs.timeline_panel_expanded }
        : {}),
      ...(prefs.editor_layout_mode !== undefined
        ? { editor_layout_mode: prefs.editor_layout_mode }
        : {}),
    },
  });
  expect(put.status()).toBe(204);
}

/** 保证后续用例从对话布局启动（避免 IDE 布局 prefs 污染聊天 E2E）。 */
export async function ensureChatLayoutPrefs(request: APIRequestContext): Promise<void> {
  await putUserPrefs(request, { editor_layout_mode: false });
}

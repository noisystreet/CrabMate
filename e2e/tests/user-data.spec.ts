import { expect, test } from '@playwright/test';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

test.describe('user-data API', () => {
  test('GET/PUT prefs round-trip on isolated CM_CRABMATE_USER_DATA_DIR', async ({ request }) => {
    const root = process.env.CM_CRABMATE_USER_DATA_DIR;
    expect(root, 'playwright webServer must set CM_CRABMATE_USER_DATA_DIR').toBeTruthy();
    expect(fs.existsSync(root!)).toBeTruthy();

    const get0 = await request.get('/user-data/prefs');
    expect(get0.ok()).toBeTruthy();

    const put = await request.put('/user-data/prefs', {
      data: {
        locale: 'en',
        theme: 'dark',
        side_panel_view: 'workspace',
        side_width: 300,
      },
    });
    expect(put.status()).toBe(204);

    const get1 = await request.get('/user-data/prefs');
    expect(get1.ok()).toBeTruthy();
    const body = (await get1.json()) as { locale?: string; theme?: string; side_width?: number };
    expect(body.locale).toBe('en');
    expect(body.theme).toBe('dark');
    expect(body.side_width).toBe(300);
  });

  test('PUT/GET current workspace sessions', async ({ request }) => {
    await request.post('/workspace', {
      data: { path: null },
    });

    const put = await request.put('/user-data/workspaces/current/sessions', {
      data: {
        sessions: [
          {
            id: 's_e2e_ud',
            title: 'E2E user-data',
            draft: '',
            messages: [],
            updated_at: 1,
            pinned: false,
            starred: false,
          },
        ],
        active_session_id: 's_e2e_ud',
      },
    });
    expect(put.status()).toBe(204);

    const get = await request.get('/user-data/workspaces/current/sessions');
    expect(get.ok()).toBeTruthy();
    const file = (await get.json()) as {
      sessions: { id: string }[];
      active_session_id?: string;
    };
    expect(file.active_session_id).toBe('s_e2e_ud');
    expect(file.sessions.some((s) => s.id === 's_e2e_ud')).toBeTruthy();
  });
});

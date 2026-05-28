import { expect, test } from '@playwright/test';

test.describe('user-data MCP API', () => {
  test.describe.configure({ mode: 'serial' });
  test('PUT assigns slug from name and GET round-trip', async ({ request }) => {
    const put = await request.put('/user-data/mcp-servers', {
      data: {
        schema_version: 1,
        global_enabled: true,
        tool_timeout_secs: 60,
        servers: [
          {
            id: 'mcp_e2e_ud',
            name: 'E2E Test Server',
            slug: '',
            command: 'true',
            enabled: false,
            created_at_ms: 0,
            updated_at_ms: 0,
          },
        ],
      },
    });
    expect(put.status()).toBe(204);

    const get = await request.get('/user-data/mcp-servers');
    expect(get.ok()).toBeTruthy();
    const body = (await get.json()) as {
      global_enabled: boolean;
      servers: {
        id: string;
        name: string;
        slug: string;
        enabled: boolean;
        has_command: boolean;
      }[];
    };
    expect(body.global_enabled).toBe(true);
    expect(body.servers).toHaveLength(1);
    expect(body.servers[0].id).toBe('mcp_e2e_ud');
    expect(body.servers[0].slug).toBe('e2e_test_server');
    expect(body.servers[0].enabled).toBe(false);
    expect(body.servers[0].has_command).toBe(true);
    expect(body.servers[0]).not.toHaveProperty('command');
  });

  test('GET status lists configured servers', async ({ request }) => {
    await request.put('/user-data/mcp-servers', {
      data: {
        schema_version: 1,
        global_enabled: false,
        tool_timeout_secs: 45,
        servers: [
          {
            id: 'mcp_e2e_status',
            name: 'Status Probe',
            slug: 'status_probe',
            command: 'true',
            enabled: true,
            created_at_ms: 0,
            updated_at_ms: 0,
          },
        ],
      },
    });

    const fileGet = await request.get('/user-data/mcp-servers');
    const fileBody = (await fileGet.json()) as { global_enabled: boolean };
    expect(fileBody.global_enabled).toBe(false);

    const status = await request.get('/user-data/mcp-servers/status');
    expect(status.ok()).toBeTruthy();
    const body = (await status.json()) as {
      global_enabled: boolean;
      tool_timeout_secs: number;
      servers: { id: string; slug: string; enabled: boolean; connected: boolean }[];
    };
    expect(body.global_enabled).toBe(false);
    expect(body.tool_timeout_secs).toBe(45);
    const row = body.servers.find((s) => s.id === 'mcp_e2e_status');
    expect(row).toBeDefined();
    expect(row!.slug).toBe('status_probe');
    expect(row!.enabled).toBe(true);
    expect(row!.connected).toBe(false);
  });
});

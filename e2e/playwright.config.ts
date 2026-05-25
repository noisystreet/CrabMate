import { defineConfig, devices } from '@playwright/test';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

const repoRoot = path.resolve(__dirname, '..');
const distIndex = path.join(repoRoot, 'frontend', 'dist', 'index.html');
const releaseBinary = path.join(repoRoot, 'target', 'release', 'crabmate');
const defaultPort = 18081;
const port = Number(process.env.E2E_PORT || defaultPort);
const baseURL = `http://127.0.0.1:${port}`;

/** CI 预编译 release 二进制时避免 webServer 再跑完整 `cargo run` 编译。 */
const serveCommand =
  process.env.CI && fs.existsSync(releaseBinary)
    ? `${releaseBinary} serve --port ${port}`
    : `cargo run --quiet -- serve --port ${port}`;
const e2eUserDataDir =
  process.env.CM_CRABMATE_USER_DATA_DIR ||
  fs.mkdtempSync(path.join(os.tmpdir(), 'crabmate-e2e-user-data-'));

/** `NO_COLOR=1` breaks some Cargo/Trunk CLIs that expect `--no-color true|false`. */
function webServerEnv(): NodeJS.ProcessEnv {
  const e = { ...process.env };
  delete e.NO_COLOR;
  delete e.CARGO_TERM_COLOR;
  return e;
}

if (!fs.existsSync(distIndex)) {
  // Fail fast with a clear message (webServer may still start; tests need the UI bundle).
  console.warn(
    `[e2e] Missing ${distIndex}\n` +
      '  Build the Leptos bundle first: cd frontend && trunk build',
  );
}

export default defineConfig({
  testDir: './tests',
  // 共享 webServer 与 CM_CRABMATE_USER_DATA_DIR；串行避免会话 prefs 交叉污染。
  fullyParallel: !process.env.CI,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL,
    trace: 'on-first-retry',
    ...devices['Desktop Chrome'],
  },
  webServer: {
    command: serveCommand,
    cwd: repoRoot,
    env: {
      ...webServerEnv(),
      CM_CRABMATE_USER_DATA_DIR: e2eUserDataDir,
      CM_E2E_FIXTURES: '1',
    },
    url: `${baseURL}/health`,
    reuseExistingServer: !process.env.CI,
    timeout: 240_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
});

import { defineConfig, devices } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';

const repoRoot = path.resolve(__dirname, '..');
const distIndex = path.join(repoRoot, 'frontend-leptos', 'dist', 'index.html');
const defaultPort = 18081;
const port = Number(process.env.E2E_PORT || defaultPort);
const baseURL = `http://127.0.0.1:${port}`;

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
      '  Build the Leptos bundle first: cd frontend-leptos && trunk build',
  );
}

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
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
    command: `cargo run --quiet -- serve --port ${port}`,
    cwd: repoRoot,
    env: webServerEnv(),
    url: `${baseURL}/health`,
    reuseExistingServer: !process.env.CI,
    timeout: 240_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
});

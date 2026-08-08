# Playwright E2E（已迁出）

浏览器 Web UI 的 Playwright 测试已迁至官方 Client 仓：

- 仓：[`noisystreet/crabmate-client`](https://github.com/noisystreet/crabmate-client)
- 目录：`e2e/`
- 一键：`cd ../crabmate-client && ./scripts/e2e-playwright.sh`
- CI：Client `.github/workflows/e2e-playwright.yml`

本仓仍保留：

- `crabmate e2e` / `tests/e2e_*`（编排与 HTTP 真 LLM）
- `CM_E2E_FIXTURES=1` 下的 HTTP `/e2e/...` 夹具路由

主仓 `./scripts/e2e-playwright.sh` 仅转发到同级 Client（若存在）。

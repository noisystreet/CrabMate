# 个人 VPS 部署指南（本机 HTTP + 反向代理 TLS + Bearer）

本文描述一种适合**个人自用**的部署方式：**CrabMate 只监听本机回环地址**（默认行为），由前面的 **Caddy（或 Nginx）终止 TLS** 并反向代理到本机端口；同时启用 **Web API 共享密钥（Bearer / `X-API-Key`）**，避免进程在公网暴露面上裸奔。

> **适用范围**：单用户、单实例、你完全信任 VPS 与工作区内容。多租户、水平扩展、按用户审计等不在本文范围；参见 **`docs/改进建议.md`**、**`docs/未来规划功能.md`**。

---

## 1. 架构与数据流

```
浏览器 --HTTPS(443)--> Caddy/Nginx --HTTP(127.0.0.1:8080)--> crabmate serve
```

- **TLS**：由反向代理处理；CrabMate 进程内**不提供**内置 HTTPS（见 **`README.md` → 部署与安全**）。
- **鉴权**：受保护路由需携带与配置一致的 **`Authorization: Bearer …`** 或 **`X-API-Key: …`**（与 **`docs/配置说明.md`**「Web API 鉴权层」一致）。浏览器侧可将同一密钥写入 **`localStorage["crabmate-api-bearer-token"]`**，与 Web UI 侧栏「设置」中的 API 共享密钥一致。
- **上游大模型**：调用厂商仍使用环境变量 **`API_KEY`**（或 Web 侧栏 `client_llm`，仅存浏览器）；**勿**把真实密钥写入仓库或本文示例。

---

## 2. 前置条件

| 项 | 说明 |
|----|------|
| 操作系统 | 常见 Linux（Debian/Ubuntu 等）；下文以 **systemd** + **Caddy** 为例。 |
| 域名（推荐） | 将 **A 记录** 指向 VPS 公网 IP，便于 **Let’s Encrypt** 自动证书。仅用 IP 访问时，自动公网证书不便签发，需自签或云厂商证书，浏览器会告警。 |
| 二进制与前端 | 已 **`cargo build --release`**，且 **`cd frontend && trunk build --release`** 后存在 **`frontend/dist`**（与 **`README.md` → 编译运行与打包** 一致）。 |
| 防火墙 | 计划只对外开放 **443**（及你管理用的 **22** SSH）；**不要**把 CrabMate 的 **8080** 直接暴露到公网。 |

---

## 3. 配置 CrabMate（Bearer + 仅本机监听）

### 3.1 推荐：环境变量（避免密钥进 TOML 文件）

在 **`systemd` 单元**或 shell 启动脚本中设置（示例均为占位符）：

```bash
export CM_WEB_API_REQUIRE_BEARER=1
export CM_WEB_API_BEARER_TOKEN='YOUR_LONG_RANDOM_SECRET'
# 可选：未使用 `serve --host` 时，绑定地址也可用环境变量覆盖
export CM_HTTP_HOST=127.0.0.1
```

生成强随机密钥示例（在 VPS 上执行一次即可）：

```bash
openssl rand -base64 32
```

### 3.2 或在 TOML 中配置

在 **`[agent]`** 段（见 **`config/default_config.toml`** 与 **`docs/配置说明.md`**）：

```toml
[agent]
web_api_require_bearer = true
web_api_bearer_token = "YOUR_LONG_RANDOM_SECRET"
```

仍建议 **`web_api_bearer_token` 仅通过本地权限控制的可写配置或密钥管理注入**，不要提交到 Git。

### 3.3 启动命令

仅本机监听（**默认即为 `127.0.0.1`**，显式写出更清晰）：

```bash
/path/to/crabmate serve --host 127.0.0.1 --port 8080
```

端口可改；若修改，下文 Caddy **`reverse_proxy`** 目标端口需一致。

> **`web_api_require_bearer = true`** 时，若启动时仍无有效非空密钥，**`serve` 会拒绝启动**（含仅监听 `127.0.0.1`）。详见 **`docs/配置说明.md`**。

---

## 4. 使用 Caddy 反向代理与 HTTPS

### 4.1 安装 Caddy

按 [Caddy 官方安装文档](https://caddyserver.com/docs/install) 使用发行版包或官方仓库安装。

### 4.2 `Caddyfile` 示例

将 **`crab.example.com`** 换成你的域名：

```
crab.example.com {
    encode zstd gzip

    # 上传附件：`serve` 侧受保护路由体上限约 220MiB（见源码 `PROTECTED_API_BODY_LIMIT_BYTES`），反代勿过小
    request_body {
        max_size 256MB
    }

    reverse_proxy 127.0.0.1:8080 {
        # 流式 SSE：降低缓冲，避免首包延迟或连接被长时间挂起时行为异常
        flush_interval -1
    }
}
```

启用配置并重载（命令因安装方式略有差异，常见为）：

```bash
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
```

浏览器访问 **`https://crab.example.com`** 即可打开 Web UI；首次需在侧栏填写 **与 `CM_WEB_API_BEARER_TOKEN` 相同** 的共享密钥并保存（或依赖已写入的 **`localStorage`**）。

### 4.3 可选：Nginx 要点

若使用 Nginx，需自行管理证书（如 **certbot**）。对流式接口建议关闭代理缓冲并放宽读超时，例如（片段，勿直接复制路径）：

```nginx
proxy_http_version 1.1;
proxy_set_header Host $host;
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
proxy_set_header X-Forwarded-Proto $scheme;
proxy_buffering off;
proxy_read_timeout 86400s;
client_max_body_size 256m;
```

具体 **`server` / `location`** 结构以你站点为准。

---

## 5. 使用 systemd 托管 `serve`（推荐）

以 **非 root** 用户 **`crabmate`** 为例（请按需替换路径与用户）：

`/etc/systemd/system/crabmate.service`：

```ini
[Unit]
Description=CrabMate Web (loopback, behind reverse proxy)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=crabmate
Group=crabmate
WorkingDirectory=/home/crabmate/crabmate
Environment=CM_WEB_API_REQUIRE_BEARER=1
Environment=CM_WEB_API_BEARER_TOKEN=YOUR_LONG_RANDOM_SECRET
Environment=CM_HTTP_HOST=127.0.0.1
Environment=RUST_LOG=info
ExecStart=/home/crabmate/bin/crabmate serve --host 127.0.0.1 --port 8080
Restart=on-failure
RestartSec=5

# 安全加固（可按需收紧）
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

加载并启动：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now crabmate.service
sudo systemctl status crabmate.service
```

> **不要把 `CM_WEB_API_BEARER_TOKEN` 写进世界可读的文件**若无法接受：改用 **`systemd` 的 `EnvironmentFile=`** 指向 **`chmod 600`** 的文件，或发行版提供的 **secrets** 机制。

---

## 6. 防火墙与安全组

- **云安全组 / 本机 `ufw`**：仅允许 **22**（SSH，若需要）、**443**（HTTPS）。**禁止**从公网访问 **8080**。
- **SSH**：优先密钥登录，关闭密码登录（按发行版文档配置）。
- **信任模型**：Bearer 为**共享密钥**，泄露即等同他人可调用你的 API；与 **`docs/待办清单.md`** 中「HTTP 身份模型」说明一致。
- **工作区**：仅挂载你愿意让 Agent 读写的目录；工具链仍可能执行 **`run_command`** 等能力，参见 **`README.md`**、**`docs/工具说明.md`**。

---

## 7. 验收与排障

| 现象 | 建议 |
|------|------|
| **`serve` 启动失败** | 检查 **`CM_WEB_API_REQUIRE_BEARER=1`** 时是否已设置非空 **`CM_WEB_API_BEARER_TOKEN`**（或 TOML 等价项）。 |
| **浏览器 401 / 无法加载会话** | 侧栏密钥是否与服务器一致；是否带 **`Authorization`/`X-API-Key`**（前端存 **`crabmate-api-bearer-token`**）。 |
| **流式对话中断** | 检查反代 **`flush_interval` / `proxy_buffering` / `proxy_read_timeout`**；中间设备超时。 |
| **上传失败** | 调大 Caddy **`request_body`** 或 Nginx **`client_max_body_size`**（建议与上节 **256MB** 量级一致，且不超过你信任的磁盘与带宽）。 |
| **证书失败** | 域名 DNS 是否指向本机；80/443 是否对 ACME 开放；查看 Caddy 日志。 |

健康检查：**`GET /health`** 与 **`GET /status`** 挂在系统路由上，**不要求** Web API Bearer（与 **`src/web/server.rs`** 路由分层一致）；探活可直接：

```bash
curl -fsS https://crab.example.com/health
```

**`POST /chat`**、**`/workspace/*`** 等受保护接口仍须携带 Bearer（或浏览器侧已配置同一密钥）。

---

## 8. 备份与升级

- **会话库**：默认在工作区 **`.crabmate/conversations.db`**（见 **`README.md`**）；定期备份该文件或整个工作区目录。
- **升级**：替换 **`crabmate` 二进制**后重启 **`systemd`**；若前端有变更，重新 **`trunk build --release`** 并部署 **`frontend/dist`**（与 **`scripts/package-release.sh`** 发行包流程一致时可一并打包）。

---

## 9. 相关文档

- **`README.md`**：部署与安全、环境变量摘要。
- **`docs/配置说明.md`**：**`CM_WEB_API_*`**、**`web_api_require_bearer`**、热重载与中间件关系。
- **`docs/命令行与路由.md`**：**`serve`** 参数与 HTTP 路由。
- **`docs/调试指南.md`**：日志 **`RUST_LOG`**、**`GET /web-ui`** 等。

---

*本文随部署实践增补；与具体发行版包名、路径以你环境为准。*

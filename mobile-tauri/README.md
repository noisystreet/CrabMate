# CrabMate Mobile（Android 远程薄客户端）

Tauri 2 壳 + 连接页，**不**拉起本机 `crabmate serve` sidecar。  
产品定位见仓库 `agent_space/tauri-android-build-plan.md`（远程方案 B）。

## Phase 1 行为

1. App 打开本地连接页：填写 **服务器 URL** + 可选 **Web API 共享密钥**（`CM_WEB_API_BEARER_TOKEN`，不是模型 `API_KEY`）。
2. 壳进程探测远程 `GET /health`（带 Bearer），失败时在连接页显示错误。
3. 成功后导航到远程 UI，并用 URL hash `#cm_web_api_bearer=…` 一次性交接密钥；远程前端启动时写入本页凭证并 `replaceState` 清掉 hash。
4. 聊天 / SSE / 工具审批均在远程 `serve` 上执行；手机侧只记住连接配置。

**注意**：远程主机须使用已包含 `consume_mobile_connect_handoff` 的前端构建（`cd frontend && trunk build` 后重启 `serve`）。

连接页只持久化服务器 URL；Web API Bearer **不落盘**（每次填写）。空 Bearer 不会写 hash，以免清掉远程源已有凭证。

`gen/android/app/build.gradle.kts` 中 release 的 `usesCleartextTraffic=true` 为局域网明文 HTTP 而设；若重新执行 `tauri android init`，需再确认该补丁仍在。公网请用 HTTPS。

## 前置

- `JAVA_HOME` 指向完整 **JDK**（需有 `javac`）
- `ANDROID_HOME` / `NDK_HOME`（本机示例：`$HOME/soft/Android/sdk`）
- Rust target：`aarch64-linux-android`
- `cargo install tauri-cli --version "^2"`

装完 JDK 后若仍报 `JAVA_COMPILER`：先 `cd gen/android && ./gradlew --stop` 再构建。

## 常用命令

仓库根目录：

```bash
make apk
make apk MOBILE_ANDROID_TARGET=aarch64 CM_MOBILE_GRADLE_STOP=1
```

开发（模拟器 / 真机）：

```bash
cd mobile-tauri/src-tauri
cargo tauri android dev
```

服务端示例（局域网）：

```bash
# 仓库根；先 trunk build 前端
CM_WEB_API_BEARER_TOKEN='your-shared-secret' cargo run -- serve --host 0.0.0.0 --port 8080
```

手机连接页填写 `http://<电脑局域网IP>:8080/` 与同一共享密钥。

## APK 产物

`gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk`

（未签名；`android dev` 可装 debug。release 已允许明文 HTTP 以便局域网；公网请用 HTTPS。）

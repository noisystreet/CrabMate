# crabmate Mobile（Android 远程薄客户端）

Tauri 2 壳 + 连接页，**不**拉起本机 `crabmate serve` sidecar。  
包名 **`edu.crabmate`**，桌面显示名 **`crabmate`**。  
产品定位见仓库 `agent_space/tauri-android-build-plan.md`（远程方案 B）。

## Phase 1 行为

1. App 打开本地连接页：填写 **服务器 URL** + 可选 **Web API 共享密钥**（`CM_WEB_API_BEARER_TOKEN`，不是模型 `API_KEY`）。
2. 壳进程探测远程 `GET /health`（带 Bearer），失败时在连接页显示错误。
3. 成功后导航到远程 UI，并用 URL hash `#cm_web_api_bearer=…` 一次性交接密钥；远程前端启动时写入本页凭证并 `replaceState` 清掉 hash。
4. 聊天 / SSE / 工具审批均在远程 `serve` 上执行；手机侧只记住连接配置。

**注意**：远程主机须使用已包含 `consume_mobile_connect_handoff` 的前端构建（`cd frontend && trunk build` 后重启 `serve`）。

连接页持久化服务器 URL；Web API Bearer **不写入 App 本地存储**，改由 **系统 Autofill / 密码管理器**（表单 `username`=`服务器地址`，`password`=`Bearer`；成功连接后 `AutofillManager.commit()`）。请在系统设置中启用自动填充服务。空 Bearer 不会写 hash，以免清掉远程源已有凭证。

`gen/android/app/build.gradle.kts` 中 release 的 `usesCleartextTraffic=true` 为局域网明文 HTTP 而设；若重新执行 `tauri android init`，需再确认该补丁与下方签名配置仍在。公网请用 HTTPS。

`MainActivity` **不**调用 `enableEdgeToEdge()`，避免 WebView 内容画进系统状态栏后与壳顶栏按钮重叠（Android WebView 一般不提供可用的 `safe-area-inset-*`）。

连上远程后：**系统返回键**或顶栏 **「断开连接」**（`window.CrabMateMobile.disconnect`）回到本地连接页；连接页再按返回则退出 App。远程源无 Tauri IPC，故断开走原生桥而非 `invoke`。

顶栏安全区：`CrabMateMobile.getStatusBarInsetPx()` 写入 CSS `--cm-safe-top`（状态栏/刘海 + 触控余量，至少约 52px）；原生还会在页面侧注入该变量。远程前端与连接页共用。

### Release 签名（可选）

本地创建（已 gitignore，勿提交）：

- `gen/android/app/key.properties`（`storePassword` / `keyPassword` / `keyAlias` / `storeFile`）
- 对应 `.jks` 密钥库（路径写在 `storeFile`）

存在 `key.properties` 时，`make apk` / `cargo tauri android build --apk` 的 release 会用该配置签名。Gradle 产物文件名为 **`crabmate.apk`**（`build.gradle.kts` 的 `outputFileName`）；`make apk` 还会复制到 **`mobile-tauri/crabmate.apk`**。无该文件时仍可打出 unsigned 包。

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
# 仅重打壳、跳过前端：make apk CM_MOBILE_SKIP_FRONTEND=1
```

`make apk` 会先 `trunk build` 前端（写入 `frontend/dist`）。手机连的是本机 **`serve`**，打完包后仍需**重启 serve** 才会用到新 UI。

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

主产物文件名：**`crabmate.apk`**

- Gradle：`gen/android/app/build/outputs/apk/universal/release/crabmate.apk`
- `make apk` 额外复制：`mobile-tauri/crabmate.apk`

（应用包名 / `applicationId` 仍是 **`edu.crabmate`**，与 APK 文件名无关。）`android dev` 可装 debug（`crabmate-debug.apk`）。release 已允许明文 HTTP 以便局域网；公网请用 HTTPS。

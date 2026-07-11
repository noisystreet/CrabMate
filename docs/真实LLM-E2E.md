# 真实 LLM E2E 自动测试（Victauri）

**`REAL_LLM_E2E=1`** 时才会执行；默认 CI / `cargo test` **不会**调用真实模型。用于验证 Tauri WebView 中流式渲染在真实厂商（如 DeepSeek OpenAI 兼容网关）下的行为。

与 [`测试指南.md`](测试指南.md) § 桌面端 E2E 的区别：**不**注入 fetch 拦截器，会消耗 API 配额，耗时可至 **数分钟～十余分钟**。

---

## 用例一览

| 文件 | 场景 | 超时（约） | 断言要点 |
|------|------|------------|----------|
| `victauri_real_llm.rs` | 单轮「你有哪些技能」 | 5 分钟 | 流式完成、助手气泡出现、无错误 |
| `victauri_real_llm.rs` | 单轮「编译 hpcg」+ 流转 | 5 分钟 | 助手回复存在、无错误 |

---

## 前置条件

1. **Rust 工具链**与仓库依赖可正常 `cargo build`。
2. **前端静态包**：`frontend/dist/index.html` 存在（须先 `trunk build`）。
3. **Tauri CLI**：`cargo install tauri-cli --version "^2"`。
4. **模型密钥**：设置 `API_KEY` 环境变量（Tauri WebView 无 localStorage，密钥需由后端进程继承）。
5. **`NO_COLOR`**：执行前 `unset NO_COLOR`。

---

## 密钥配置（推荐环境变量 `API_KEY`）

```bash
export API_KEY="YOUR_DEEPSEEK_API_KEY"
```

`serve` 进程设有 `API_KEY` 时，所有经 `/chat/stream` 的请求使用该密钥作为默认值。

---

## 运行

```bash
# 终端 1：启动 Tauri 桌面应用
cd desktop-tauri/src-tauri
CM_DESKTOP_BACKEND_BIN=/path/to/target/debug/crabmate cargo tauri dev

# 终端 2：运行真实 LLM 测试
cd desktop-tauri/src-tauri
VICTAURI_E2E=1 CM_E2E_FIXTURES=1 REAL_LLM_E2E=1 cargo test --test victauri_real_llm -- --nocapture
```

---
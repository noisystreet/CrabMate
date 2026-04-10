# 活文档模板（复制到工作区）

将本目录下的文件复制到工作区 **`.crabmate/living_docs/`**（与 `.gitignore` 中的本机数据目录一致），然后在配置中开启 **`living_docs_inject_enabled`**（见 **`config/context_inject.toml`** / **`AGENT_LIVING_DOCS_*`**）。

可选文件名（服务端按此顺序拼接摘要）：

- **`SUMMARY.md`**：给新会话的短摘要（模块边界、当前焦点）。
- **`map.md`**：模块/目录地图。
- **`pitfalls.md`**：常见坑与排障要点。
- **`build.md`**：常用构建与测试命令（勿写密钥）。

细节见 **`docs/CONFIGURATION.md`**「首轮注入」与 **`docs/DEVELOPMENT.md`** 中 **`living_docs.rs`**。

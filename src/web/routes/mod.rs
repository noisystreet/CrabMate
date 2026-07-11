//! 按业务域划分的 HTTP 路由：与对应 JSON 请求/响应类型同目录，便于对照维护。
//!
//! | 子模块 | 路径前缀（相对站点根） | 说明 |
//! |--------|------------------------|------|
//! | [`chat`] | `/chat*`、`/upload*`、`/uploads/delete` | 对话与上传（handler 在 `chat_handlers`） |
//! | [`config`] | `/config/reload` | 配置热重载 |
//! | [`workspace`] | `/workspace*` | 工作区浏览、文件、changelog |
//! | [`tasks`] | `/tasks` | 侧栏任务清单（内存） |
//! | [`user_data`] | `/user-data/*` | 本机用户数据（prefs、会话桶、LLM 覆盖、secrets） |
//! | [`system`] | `/health`、`/status` | 探活与运行态摘要 |
//! | [`e2e_fixtures`] | `/e2e/fixtures/*` | 仅 **`CM_E2E_FIXTURES=1`** 时挂载（Victauri E2E 种子会话） |

pub(crate) mod chat;
pub(crate) mod config;
pub(crate) mod e2e_fixtures;
pub(crate) mod system;
pub(crate) mod tasks;
pub(crate) mod user_data;
pub(crate) mod workspace;

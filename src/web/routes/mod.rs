//! 按业务域划分的 HTTP 路由：与对应 JSON 请求/响应类型同目录，便于对照维护。
//!
//! | 子模块 | 路径前缀（相对站点根） | 说明 |
//! |--------|------------------------|------|
//! | [`chat`] | `/chat*`、`/upload*`、`/uploads/delete` | 对话与上传（handler 在 `chat_handlers`） |
//! | [`config`] | `/config/reload` | 配置热重载 |
//! | [`workspace`] | `/workspace*` | 工作区浏览、文件、changelog |
//! | [`tasks`] | `/tasks` | 侧栏任务清单（内存） |
//! | [`system`] | `/health`、`/status` | 探活与运行态摘要 |

pub(crate) mod chat;
pub(crate) mod config;
pub(crate) mod system;
pub(crate) mod tasks;
pub(crate) mod workspace;

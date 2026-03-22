//! Agent 流式下行（SSE 行）：有界用于 Web 背压，无界用于 TUI（避免 UI 绘制慢时 `send().await` 卡住整条 Agent 任务）。

use tokio::sync::mpsc;

/// 与 `run_agent_turn` / `stream_chat` 等配合的引用形式发送端。
#[derive(Clone, Copy)]
pub enum AgentStreamOut<'a> {
    Bounded(&'a mpsc::Sender<String>),
    Unbounded(&'a mpsc::UnboundedSender<String>),
}

impl AgentStreamOut<'_> {
    pub async fn send(&self, line: String) -> Result<(), mpsc::error::SendError<String>> {
        match self {
            Self::Bounded(tx) => tx.send(line).await,
            Self::Unbounded(tx) => tx.send(line),
        }
    }

    pub fn is_closed(&self) -> bool {
        match self {
            Self::Bounded(tx) => tx.is_closed(),
            Self::Unbounded(tx) => tx.is_closed(),
        }
    }

    /// 供 TUI 工具运行时克隆进子任务（审批、workflow 等）。
    pub fn to_owned_tx(self) -> AgentStreamOutTx {
        match self {
            Self::Bounded(tx) => AgentStreamOutTx::Bounded(tx.clone()),
            Self::Unbounded(tx) => AgentStreamOutTx::Unbounded(tx.clone()),
        }
    }
}

/// 拥有型发送端，可在 `tokio::spawn` / workflow 节点间 `clone`。
#[derive(Clone, Debug)]
pub enum AgentStreamOutTx {
    Bounded(mpsc::Sender<String>),
    Unbounded(mpsc::UnboundedSender<String>),
}

impl AgentStreamOutTx {
    pub async fn send(&self, line: String) -> Result<(), mpsc::error::SendError<String>> {
        match self {
            Self::Bounded(tx) => tx.send(line).await,
            Self::Unbounded(tx) => tx.send(line),
        }
    }

    pub fn is_closed(&self) -> bool {
        match self {
            Self::Bounded(tx) => tx.is_closed(),
            Self::Unbounded(tx) => tx.is_closed(),
        }
    }
}

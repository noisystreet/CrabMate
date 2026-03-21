//! CrabMate 可执行文件入口：调用库内 [`crabmate::run`]。

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    crabmate::run().await
}

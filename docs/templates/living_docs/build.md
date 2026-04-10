## 构建与测试（示例）

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p crabmate
cd frontend-leptos && trunk build
```

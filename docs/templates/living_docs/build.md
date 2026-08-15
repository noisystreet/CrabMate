## 构建与测试（示例）

业务 UI 构建在 [crabmate-client](https://github.com/noisystreet/crabmate-client)（本机命令假定同级克隆）：

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p crabmate
cd ../crabmate-client && make frontend
```

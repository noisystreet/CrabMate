# Agent Demo 前端

基于 **Vite + React + TypeScript + Tailwind CSS** 的聊天界面。

## 开发

```bash
npm install
npm run dev
```

开发时 Vite 会代理 `/chat`、`/status`、`/workspace`、`/health` 到后端（默认 `http://127.0.0.1:8080`），请先启动后端：

```bash
# 在项目根目录
cargo run -- serve 8080
```

## 构建与部署

```bash
npm run build
```

构建产物在 `frontend/dist`。后端会优先从 `frontend/dist` 提供静态资源（若存在），否则使用根目录下的 `static/`。

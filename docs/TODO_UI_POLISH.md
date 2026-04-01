# 前端美化专项 TODO

> 范围：`frontend-leptos` 的视觉与交互优化（不改业务协议）。
> 目标：在现有样式基础上，持续提升层次感、一致性、可读性与可访问性。

## 验收建议（每次改动）

- [ ] `cd frontend-leptos && cargo check --target wasm32-unknown-unknown`
- [ ] 手动验证 dark / light 两套主题
- [ ] 手动验证窄屏（顶栏、侧栏、状态栏不重叠）
- [ ] 手动验证骨架屏到真实内容的切换是否平滑
- [ ] 对照 [`docs/frontend-leptos/VISUAL_REGRESSION_CHECKLIST.md`](frontend-leptos/VISUAL_REGRESSION_CHECKLIST.md) 做关键路径手测

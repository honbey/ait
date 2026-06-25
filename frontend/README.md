# Ait Frontend

基于 Sycamore 0.9 的 WASM CSR 前端，目前是编译为 WebAssembly 后嵌入 Ait 后端。

## 技术栈

| 层 | 选型 |
|---|---|
| 框架 | [Sycamore 0.9](https://sycamore.dev/)（响应式信号驱动）|
| 路由 | [sycamore-router 0.9](https://docs.rs/sycamore-router/0.9) |
| 样式 | [Tailwind CSS 4](https://tailwindcss.com/) |
| 图表 | [ECharts 6.1](https://echarts.apache.org/)（动态注入）|
| 构建 | [Trunk](https://trunkrs.dev/) |
| HTTP | [gloo-net 0.7](https://docs.rs/gloo-net/0.7) |
| 国际化 | compile-time i18n（`locales/`）|

## 页面

| 路径 | 页面 | 说明 |
|------|------|------|
| `/` | 首页 | - |
| `/login` | 登录 | 用户名密码登录 |
| `/register` | 注册 | 受控注册（需邀请码）|
| `/dashboard` | 概览 | API 请求数 / Token 消耗 |
| `/providers` | 提供商管理 | - |
| `/models` | 模型管理 | - |
| `/api-keys` | API Key 管理 | - |
| `/text-generation` | 文本生成 | 文本生成接口，需要 API KEY |

## 开发

```bash
# 开发模式（自动重编译）
cd frontend && trunk watch

# 生产构建
trunk build --release --cargo-profile release-wasm
```

## ECharts

ECharts 6.1 完整构建（~1.1MB）位于 `assets/echarts/echarts.min.js`，
通过 Trunk `copy-file` 部署到 `/echarts.min.js`。
前端在 `LineChart` 组件挂载时动态创建 `<script>` 标签注入，非 Dashboard 页面不会加载。

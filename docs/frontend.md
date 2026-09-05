# Ait 前端

基于 Leptos 0.8 CSR 的 WASM 前端，编译的 WebAssembly 等静态文件由 Ait 后端 tower-http 的 `ServeDir` 提供。

## 技术栈

| 层 | 选型 |
| --- | --- |
| 框架 | [Leptos 0.8](https://leptos.dev/)（CSR 模式） |
| 路由 | [leptos_router 0.8](https://docs.rs/leptos_router/0.8) |
| 样式 | [Tailwind CSS 4](https://tailwindcss.com/)（Trunk 构建钩子） |
| 图表 | [ECharts 6.1](https://echarts.apache.org/)（动态注入） |
| 构建 | [Trunk 0.21.14](https://trunkrs.dev/) |
| HTTP | [gloo-net 0.7](https://docs.rs/gloo-net/0.7) |
| 存储 | [gloo-storage 0.4](https://docs.rs/gloo-storage)（Local → Session → Memory 回退链） |
| 格式化 | [leptosfmt](https://github.com/bram2103/leptosfmt)（`cargo fmt` 不会格式化 view! 宏） |
| 国际化 | compile-time i18n（`locales/lang.json` → `build.rs` 生成 `K` 枚举） |

## 功能

- **后台管理** — 管理提供商/模型/API Key
- **多语言** — 默认跟随浏览器语言（zh/en，其他回退 zh），仅显式切换才持久化到 localStorage
- **深色模式** — 无存储偏好时跟随系统主题（`prefers-color-scheme`），仅显式切换才持久化
- **骨架屏加载** — 控制台页面数据加载中显示占位骨架
- **全局 Toast** — 会话过期、操作结果等全局通知
- **Docs / About** — 文档中心与关于页面

## 页面

| 路径 | 页面 | 说明 |
| ------ | ------ | ------ |
| `/` | 首页 | - |
| `/docs` | 文档中心 | 项目与前端文档入口 |
| `/about` | 关于 | 项目信息 |
| `/console/overview` | 概览 | 提供商 / 模型总数、API 请求数 / Token 消耗图表 |
| `/console/providers` | 提供商管理 | - |
| `/console/models` | 模型管理 | 支持按提供商筛选 |
| `/console/api-keys` | API Key 管理 | - |
| `/console/logs` | 日志 | 代理请求日志查询与统计 |

## 开发

```bash
# 开发模式（自动重编译）
cd frontend && trunk watch

# 格式化 Leptos view! 宏（cargo fmt 不会处理）
leptosfmt src/

# 生产构建
trunk build --release --cargo-profile release-wasm
```

## ECharts

ECharts 6.1 完整构建（~1.1MB）位于 `assets/echarts-6.1.0/echarts.min.js`，
通过 Trunk `copy-file` 部署。前端在 `LineChart` 组件挂载时动态创建 `<script>` 标签注入，非概览页面不会加载。

## 深色模式配色

暗色模式使用 `tailwind.css` 中定义的中性 `ink` 色板，而非 Tailwind 内置的 `gray`
——后者色相偏蓝（色相约 260），在暗色表面上呈海军蓝而非黑灰。`ink` 各档的 `oklch`
亮度与 `gray` 逐档对齐、色度归零，因此文字对比度不变，仅去掉蓝味；暗色表面在此基础上再降一档以拉开层次。

图表（ECharts）的暗色配色是硬编码十六进制值，集中在 `components/line_chart.rs` 与
`components/pie_chart.rs`，修改色板时需同步调整。

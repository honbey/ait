# Ait Frontend

基于 Leptos 0.8 CSR 的 WASM 前端，编译的 WebAssembly 等静态文件由 Ait 后端 tower-http 的 `ServerDir` 提供。

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
| `/docs` | 文档中心 | 项目与前端 README 入口 |
| `/about` | 关于 | 项目信息 |
| `/console/overview` | 概览 | 提供商 / 模型总数、API 请求数 / Token 消耗图表 |
| `/console/providers` | 提供商管理 | - |
| `/console/models` | 模型管理 | - |
| `/console/api-keys` | API Key 管理 | - |
| `/console/logs` | 日志 | 代理请求日志查询与统计 |
| `/console/text-generation` | 文本生成 | 文本生成接口 demo，需要 API Key |

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

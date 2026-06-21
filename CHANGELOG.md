# 更新日志

## [v0.1.2] - 2026-06-21

### 新增功能

- **API Key 管理** — 前端完整 CRUD 页面（创建、启用/禁用、删除），支持创建时设定过期时间或永不过期，快速复制到剪贴板
- **文本生成页面** — 新页面通过 `/v1/completions` 代理接口提交生成请求，支持 Temperature/MaxTokens/TopP 参数调节（range 滑块）
- **仪表盘统计接口** — `GET /admin/stats/dashboard` 返回提供商/模型数量及预留的 API 调用次数/Token 消耗字段
- **用户注册** — `POST /auth/register` 允许新用户注册，受 `allow_registration` 和 `registration_code` 配置控制
- **配置项更新**
  - 移除旧版 `auth.token` 和 `auth.admin_token`
  - 新增 `server.session_cleanup_interval_secs`、`auth.session_ttl_secs`、`auth.allow_registration`、`auth.registration_code`、`auth.max_api_keys_per_user`

### 重构优化

- **后端**
  - 提取通用 HTTP 请求辅助函数，统一 `AitError` 错误类型
  - Admin 接口拆分为独立模块（`admin.rs`、`apikeys.rs`、`users.rs`、`stats.rs`）
  - `SessionUser` 重构，角色使用 `UserRole` 枚举
  - `DbError` 统一处理模式，日期固定使用 UTC

- **前端**
  - 提取共享组件 `data_table.rs`、`delete_confirm.rs`、`modal.rs`，减少约 40% 重复代码
  - API 调用集中到 `api.rs`，统一 `NetError` 处理
  - Provider/Model/API Key 管理页面全部重构为共享组件模式
  - 所有表单字段添加 `id`/`for` 属性提升无障碍访问
  - `cargo fmt` 全量格式化 + Clippy 告警清理

### 修复

- **登录后信号刷新** — 登录成功后立即更新 `username`/`role` 响应式信号，避免 UI 不同步
- **登出闭包递归** — 修复登出时因闭包递归调用导致的 panic

## [v0.1.1] - 2026-06-17

### 新增功能

- **Web 管理界面** — 基于 Sycamore 0.9 的 WASM CSR 前端
  - 6 个路由页面：首页、登录、仪表盘、提供商、模型、文本生成
  - 响应式侧边栏 + 移动端浮动按钮
  - i18n 中英文切换
  - 深色模式切换（localStorage 持久化）
  - Provider/Model 表格及详情弹窗

- **会话认证系统**
  - bcrypt 密码哈希存储
  - HttpOnly Cookie 存储 session key（XSS 防护）
  - 首次启动自动创建管理员（`bootstrap_admin`）
  - Admin 中间件支持三种认证：静态 Token（Bearer）、Session Key（Bearer）、Session Key（Cookie）

## [v0.1.0] - 2026-06-10

### 新增功能

- **代理 API（OpenAI 兼容）**
  - `POST /v1/chat/completions` — 聊天补全（支持流式和非流式）
  - `POST /v1/completions` — 文本补全
  - `POST /v1/embeddings` — 嵌入
  - `GET /v1/models` — 模型列表
  - `GET /v1/health` — 健康检查，设有详细模式

- **管理 API**
  - `POST /admin/providers` — 创建提供商
  - `GET /admin/providers` — 列出提供商（API Key 脱敏）
  - `GET /admin/providers/{id}` — 获取提供商（API Key 脱敏）
  - `GET /admin/providers/{id}/api-key` — 获取完整 API Key
  - `PUT /admin/providers/{id}` — 更新提供商
  - `DELETE /admin/providers/{id}` — 删除提供商（级联删除模型）
  - `POST /admin/models` — 创建模型
  - `GET /admin/models` — 列出模型
  - `DELETE /admin/models/{name}` — 删除模型

- **多提供商支持**
  - OpenAI 兼容接口（含 DeepSeek、Zhipu、LlamaCpp）
  - Ollama（路径映射、字段透传、reasoning_effort 转换）

- **认证**
  - Proxy 接口和 Admin 接口使用不同的 Token
  - Admin 接口始终需要认证（不受 auth.enabled 影响）

- **配置**
  - 配置文件为 Toml 格式，`-c` / `--config` 命令行参数指定配置文件
  - 环境变量覆盖（`AIT_<SECTION>_<KEY>`）

- **存储**
  - 使用 RocksDB 持久化存储提供商和模型信息

### 安全

- Admin 接口由独立 admin_token 保护且必须认证
- 提供商列表/详情接口中 API Key 自动脱敏，使用专用接口获取完整 API Key

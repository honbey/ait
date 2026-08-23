# 更新日志

## [v0.2.0] - 2026-08-23

### Breaking Changes

- **移除内置用户/密码/会话系统** — 删除 `users`、`sessions` 表，移除 `bootstrap_*`、`session_ttl_*`、`max_api_keys_per_user`、`login_rate_limit` 等配置项
- **管理接口鉴权外置** — `/api/*` 不再由 Ait 自身鉴权，由外部反向代理（如 nginx + Authelia）保护
- **API Key 路径变更** — 从 `/api/users/{username}/api-keys` 改为 `/api/api-keys`（全局管理，不再按用户隔离）
- **日志表删除 `username` 列** — DuckDB `access_log`、`proxy_log`、`audit_log` 不再记录用户名；`proxy_log` 保留 `api_key_name`
- **`/auth/session` 接口删除** — 管理鉴权完全由反向代理处理，`Remote-User` header 仅用于 overview 问候语
- **前端移除登录页** — 管理界面不再内置登录/改密功能

### 新增功能

- **部署文档** — 新增 `docs/auth-proxy.md`，提供 nginx + Authelia 配置示例
- **回归测试** — 新增 `/api/*` 无鉴权可达、`/v1/*` 需 API Key 的端到端验证用例

### 移除

- 删除 `src/handlers/users.rs`、`src/rate_limiter.rs`
- 移除 `bcrypt` 依赖

### Chore

- 前端 `SessionExpired` 改为 `ApiKeyExpired`
- `config/ait.toml.example` 更新 `cors_allow_credentials` 注释
- README 默认配置示例与当前配置同步

## [v0.1.8] - 2026-08-05

### 新增功能

- **请求体 DLP 检测** - 新增 `security.dlp` 配置（`enabled` + `sensitive_values`），请求体 JSON 字符串值命中配置敏感字面量（子串匹配、区分大小写）即阻断请求，被掩码的敏感值记录在代理日志 `error_message`
- **请求体大小上限** - 新增 `proxy.max_request_body_bytes`（默认 8MB），超出直接返回 400

### 修复

- **代理日志时间戳** - 后端返回秒级时间戳，前端误按微秒换算导致时间显示错误

### Chore

- README 与示例配置补齐实际配置项（`max_response_body_bytes`、`max_request_body_bytes`、`cors_allow_credentials`、`security.dlp`、`connect_timeout_secs`、`sse_idle_timeout_secs`）

## [v0.1.7] - 2026-08-03

### 新增功能

- **Docs / About 页面** - 新增文档中心与关于页面，文档中心提供项目与前端 README 入口
- **改密码弹窗** - 用户下拉菜单新增修改密码入口，成功后自动登出并跳转登录页
- **主题跟随系统** - 无存储偏好时按系统主题（`prefers-color-scheme`）初始化，仅显式切换才写入 localStorage
- **语言跟随浏览器** - 无存储偏好时按浏览器语言初始化（`zh*` → zh，`en*` → en，其他回退 zh），仅显式切换才持久化
- **登录表单自动填充** - 用户名/密码输入框补充 `autocomplete` 属性，恢复浏览器自动填充
- **CORS 凭据支持** - 新增 `cors_allow_credentials` 配置项，支持跨域部署场景

### 修复

- **API Key** - `expires_at=0` 视为永不过期；部分更新跳过未设字段；按字符数而非字节索引掩码 key
- **代理** - 限制上游响应体大小并超时读取错误体；model_cache 增长受限且负条目快速过期
- **SSRF 防护** - 校验前规范化 IPv4-mapped 与 NAT64 IPv6 地址，防止绕过白名单
- **会话认证** - cookie 续期实现滑动过期；改密码后清理会话缓存；修复会话 key 泄漏与 SSE chunk 边界顺序
- **日志** - 拒绝 `retention_every=0` 并增加防 panic 保护；代理日志查询参数 URL 编码
- **并发** - 修复 rate_limiter 容量满时驱逐导致的死锁
- **前端** - 修复手工测试发现的 11 个 bug；API Key 创建后重新拉取列表而非前端掩盖

### 优化

- **测试补全（+2600 行）**
  - 业务集成测试：apikeys / auth / models / providers / users handlers
  - 基础设施单测：config / app / main / error / rate_limiter / blocking
  - DuckDB 日志与分析：logger / analytics 及 logs / stats / analytics handlers
  - DashMap 防死锁：`assert_no_deadlock` helper、生产模式镜像与并发压力测试、positive control 验证检测能力
  - 全量测试 169 个全部通过
- **可测性重构** - 抽取 `parse_config_path_from` 与 `cleanup_caches` 便于单元测试
- **测试稳定性** - DuckDB logger 测试改为 `shutdown` join 后断言，消除并行执行下的时序竞态

### Chore

- 前端主题 / 语言存储策略与系统偏好对齐（仅在用户显式切换时持久化）

## [v0.1.6] - 2026-07-28

### 新增功能

- **X-Request-Id 追踪** - 所有响应添加 `X-Request-Id` header，`access_log` / `proxy_log` / `audit_log` 中记录 `request_id`，方便关联请求全链路
- **分页条数选择** - 日志表格增加 10/20/50 每页条数下拉选择器

### 优化

- **前端响应式优化**
  - 图表数据从 `RwSignal+Effect` 迁移至 `Memo`，减少冗余渲染
  - `use_page_title` 改为响应式，切换语言时自动更新 `<title>`
- **UI 统一**
  - 概览与日志页面的日期选择合并为范围选择器，对齐网格布局
  - 默认时间范围改为最近 7 天，end_ts 设为当天 23:59:59 覆盖完整一天

### Chore

- Release workflow 改为 `softprops/action-gh-release`，tag 推送时自动创建 GitHub Release
- 前端时间工具函数补充 UTC/local timezone 语义注释

## [v0.1.5] - 2026-07-22

### 新增功能

- **SSE 空闲超时** - 流式响应超时返回 504，防止挂起的流占用资源
- **base_url 安全校验** - 创建/更新 provider 时校验 URL 格式并预检 SSRF
- **端点类型校验** - 按 provider type 拒绝不支持的端点
- **会话自动续期** - 活跃访问时自动续期 session 过期时间
- **run_blocking 超时保护** - 30s 超时防止阻塞任务卡死

### 优化

- **死锁修复** - DashMap 读锁在 .await 前释放，防止自死锁
- **代理**
  - body 以 owned value 传递到 build_request 消除 clone
  - 提取 collect_x_headers 和 redirect_error 公共辅助函数
  - body_len 估算替代 tokenizer
- **缓存**
  - 命中时续期 TTL + 容量驱逐
  - 精确失效替代 clear() 全量清除
- **前端**
  - AuthStatus 三态枚举替代 Option\<bool\>
  - 骨架屏与实际页面布局对齐
  - keyed diff patch 替代 clone_from 全量同步
  - ECharts 持久化组件 + ResizeObserver 优化生命周期
  - 提取 FormModalShell / use_page_title 消除重复代码
  - 硬编码 CSS 类提取到 style.rs 常量
  - 分页器重新设计为紧凑布局
  - 日志筛选表单添加 id/for 无障碍属性
  - 本地时区感知的 UTC 时间戳计算

### 修复

- **CORS** - 策略收紧
- **数据库** - insert_model / insert_api_key 包裹 BEGIN IMMEDIATE
- **上游错误** - 清理 proxy 和 SSRF 中的错误信息，UTF-8 安全截断
- **输入验证** - provider / model / apikey 字符串字段验证，拒绝纯数字 host
- **路由** - 未注册路由返回 404 而非 SPA fallback
- **HTTP 状态码** - DbError 映射正确状态码，创建返回 201，删除不存在返回 404
- **时间戳** - ProviderResponse / ModelResponse 时间戳类型对齐
- **前端** - Action value effects 防过期重复触发，创建后 mask API key，401 清除缓存

### Chore

- 移除 llama.cpp reasoning_effort 映射
- i18n 清理未使用 key，排序统一

## [v0.1.4] - 2026-06-30

### 新增功能

- **llama.cpp 思考模式** - 支持 `reasoning_effort` -> `chat_template_kwargs` 转换
- **Ollama 思考兼容** - 响应中 `reasoning` -> `reasoning_content` 转换
- **`AppInitError` 类型安全退出** - 配置校验失败时替代 `process::exit`
- **前端页面标题/语言动态化** - `<title>` 随路由切换（如 "Ait - 概览"），`<html lang>` 同步界面语言变化
- **前端 401 自动跳转** - 会话过期时前端自动重定向到登录页并弹出提示
- **前端渲染**
  - **骨架屏** - 控制台页面加载中显示占位骨架屏，替代全屏 SVG spinner，减少布局抖动
  - **路由过渡动画** - 页面切换时内容区 0.2s 淡入 + 轻微上滑
  - **概览图表日期补全** - 折线图自动填充无数据日期，避免 x 轴跳空
  - **全局 Toast 通知** - 独立 Toast 组件，支持 i18n，使用 Keyed 列表以便 DOM diff 优化
  - **操作按钮繁忙状态** - 提交、保存等按钮操作中显示旋转图标，防止重复提交表单

### 优化

- **安全性**
  - 登录/注册恒定时间比较，消除时序侧信道攻击面
  - API Key 缓存（TTL 300s），减少 RocksDB 查询
  - 速率限制器使用配置值替代硬编码
  - DashMap 条目上限，防止内存耗尽
  - `X-Forwarded-For` 限制为可信代理 IP
- **性能**
  - 使用 `spawn_blocking` 异步阻塞策略处理耗时操作（写、批量读），单次 `get_cf` (~10–50µs) 不使用，避免不必要的线程切换
  - Auth header 预格式化 + `X-Forwarded-For` 可信代理列表预解析
  - SSE `Vec<u8>` -> `BytesMut`，消除双拷贝
  - `serde_json::to_string` -> `to_vec`（跳过 UTF-8 校验）
  - 只计数查询 `count_providers` / `count_models` / `has_any_users` 替代全量反序列化
  - 前端 `View::from_dynamic` 从 27 处减至 14 处
  - 前端图表 `serde_json` -> `serde-wasm-bindgen` 省去 String 中转
  - 前端表单信号归并、Tailwind class 常量提取、`Rc<Vec>` 避免深拷贝
- **路由重构**
  - Admin 接口从 `/admin/` 迁移至 `/api/` 前缀下
  - Proxy 模块拆分为 `mod` / `guard` / `sse` / `exec` 子模块
  - 前端 `<a>` 全量替换为 SPA 客户端导航
- **代码维护**
  - 移除 `UserRole` / `Permission` 层级体系（所有用户等价），简化权限模型
  - Provider type 枚举改用 `strum` 派生宏，移除手写 helper
  - `AitError` 构造函数去重 + `into_response()` 内联
  - 依赖裁剪：tokio 从 `full` 到 7 个必需 feature，移除未使用的 `reqwest/json`
  - 前端 `data_table.rs` 合并入 `table.rs`

### 修复

- **LogManager 关闭死循环** - DuckDB `CHECKPOINT` 失败时陷入无限循环
- **SSE 断开日志丢失** - 客户端断开后代理日志未写入
- **delete_user 索引遗漏** - 删除用户未同步清理 `SESSION_EXPIRY_CF` 条目
- **深色模式颜色不匹配** - API Key 过期时间在深色模式下颜色异常
- **未登录网络错误闪烁** - 网络异常时骨架屏闪烁到登录页
- **导航竞态** - 路由切换后旧数据短暂闪现
- **前端内存泄漏** - Clipboard / Timeout / Closure 未正确释放

## [v0.1.3] - 2026-06-25

### 新增功能

- **日志**
  - 三张日志表：`access_log`（访问日志）、`proxy_log`（代理请求，含 Token 用量）、`audit_log`（操作审计）
  - 后台线程批量写入，可配置 `flush_batch`（默认 100）和 `flush_interval_secs`（默认 10s）
  - `retention_days` 自动清理过期日志同时 `CHECKPOINT` 确保 WAL 数据写入数据库
  - Analytics 独立线程通过 channel 处理聚合查询，不阻塞 Tokio 运行时
  - `admin/login`、`admin/register` 等敏感接口埋入审计事件
  - 使用 DuckDB 作为日志存储和日志后端，依赖: `duckdb = "1.10504.0"`（bundled + chrono）
- **SSE 流式 Token 追踪**
  - `UsageTrackingStream` 包装上游字节流，结束前解析最后一个 SSE 事件提取 `usage`
  - `stream_options.include_usage` 自动注入 OpenAI/Ollama 请求体
  - `parse_sse_usage` 支持 OpenAI SSE 和 Ollama NDJSON 格式
  - 非流式响应同步解析 `usage` 字段
- **前端 ECharts 图表**
  - 前端 WASM 集成 ECharts 6.1，动态注入 `<script>` 延迟加载
  - `LineChart` 组件带挂载保护 + 超时清理，防止信号竞争
- **启动自动创建管理员** - 检测到无 admin 用户时自动创建，不再依赖 `bootstrap_admin` 开关
- **登录/注册限流** - `login_rate_limit` 和 `register_rate_limit` 可配置（attempts/window/ban），`RateLimiter` 基于 `dashmap`
- **提供商类型查询** - `GET /admin/provider-types` 返回支持的提供商类型列表
- **上游代理优化** - 透传 `x-*` 自定义请求头和 `retry-after` 响应头
- **Admin 保护** - 删除/降级用户时检查 `count_admins() <= 1`，拒绝移除最后的管理员

### 重构优化

- **配置项**
  - 移除 `auth.bootstrap_admin`（始终自动创建）
  - `auth.bootstrap_password` 改为 `Option<String>`，未设置且需创建时启动报错退出
  - 新增 `server.session_cleanup_interval_secs`、`server.rate_limiter_cleanup_interval_secs`、`auth.login_rate_limit`、`auth.register_rate_limit`
- **代码提取**
  - `create_user()` 提取到 `handlers/users.rs`，统一 bcrypt 哈希 + 用户创建
  - `Database` 模块拆分为 `db/` 子目录：`store.rs`、`models.rs`、`logger.rs`、`analytics.rs`
  - Analytics `*_impl` 移除冗余条件分支，始终 `WHERE timestamp >= ?1`
  - Ollama 移除路径映射和字段转换，直接使用 OpenAI 兼容路径
- **数据库写入原子性** - `delete_user` 等操作改用 `WriteBatch` 保证多 CF 写入原子性
- **前端重构**
  - 提取 `auth_form.rs` 共享组件，登录/注册页面复用
  - 提取 `storage.rs` 模块，统一 localStorage 操作
  - Sidebar/Topbar/Modal/ProviderTable/ModelTable/ApiKeyTable/StatCard 全面组件化
  - 文本生成独立为路由页面，替换原来的组件内嵌模式
  - i18n 编译期安全：locale key 编译时校验，移除运行时fallback
  - URL 路由改造：统一使用 `AppRoute` 枚举 + `navigate()`，移除 `window.location` hack
  - 登录/登出使用响应式信号驱动，移除手动页面刷新

### 修复

- **前端导航竞态** - 概览页面资源 `match` 加 `current_route` 守卫，防旧数据闪烁
- **其他** - 见 commit 信息

## [v0.1.2] - 2026-06-21

### 新增功能

- **API Key 管理** - 前端完整 CRUD 页面（创建、启用/禁用、删除），支持创建时设定过期时间或永不过期，快速复制到剪贴板
- **文本生成页面** - 新页面通过 `/v1/completions` 代理接口提交生成请求，支持 Temperature/MaxTokens/TopP 参数调节（range 滑块）
- **概览统计接口** - `GET /admin/stats/dashboard` 返回提供商/模型数量及预留的 API 调用次数/Token 消耗字段
- **用户注册** - `POST /auth/register` 允许新用户注册，受 `allow_registration` 和 `registration_code` 配置控制
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

- **登录后信号刷新** - 登录成功后立即更新 `username`/`role` 响应式信号，避免 UI 不同步
- **登出闭包递归** - 修复登出时因闭包递归调用导致的 panic

## [v0.1.1] - 2026-06-17

### 新增功能

- **Web 管理界面** - 基于 Sycamore 0.9 的 WASM CSR 前端
  - 6 个路由页面：首页、登录、概览、提供商、模型、文本生成
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
  - `POST /v1/chat/completions` - 聊天补全（支持流式和非流式）
  - `POST /v1/completions` - 文本补全
  - `POST /v1/embeddings` - 嵌入
  - `GET /v1/models` - 模型列表
  - `GET /v1/health` - 健康检查，设有详细模式

- **管理 API**
  - `POST /admin/providers` - 创建提供商
  - `GET /admin/providers` - 列出提供商（API Key 脱敏）
  - `GET /admin/providers/{id}` - 获取提供商（API Key 脱敏）
  - `GET /admin/providers/{id}/api-key` - 获取完整 API Key
  - `PUT /admin/providers/{id}` - 更新提供商
  - `DELETE /admin/providers/{id}` - 删除提供商（级联删除模型）
  - `POST /admin/models` - 创建模型
  - `GET /admin/models` - 列出模型
  - `DELETE /admin/models/{name}` - 删除模型

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

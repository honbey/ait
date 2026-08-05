# Ait

本项目是为了将不同大模型提供商提供的 API 统一管理，
用户就可以通过一个 `base_url` 和 `api_key` 访问不同提供商提供的各种模型。

主要是代理请求到各提供商的 [OpenAI API](https://developers.openai.com/api/reference/overview) 格式的接口，
在这个过程中记录请求数、Token 用量、访问日志等方便用户审计。

## 特性

- **多提供商支持** — [llama.cpp](https://llama.app/)、[Ollama](https://ollama.com)、[DeepSeek](https://www.deepseek.com/)、[Zhipu](https://bigmodel.cn/) 等提供了 OpenAI Compatible 接口的
- **Web 管理界面** — 基于 Leptos 0.8 CSR 的 WASM 前端（ECharts 图表）
  - **Admin 管理** — 添加/更新/删除提供商和模型
  - **API Key 管理** — 创建/启用/禁用/删除 API Key
  - **Docs / About** — 文档中心与关于页面
  - **骨架屏加载** — 页面切换时显示占位骨架屏，减少布局抖动
  - **全局 Toast** — 会话过期、操作结果等全局通知
- **日志审计** — 访问日志（含 `X-Request-Id` 全链路追踪）、代理请求日志（含 Token 用量）、敏感操作审计日志，支持自动清理和统计分析

## 快速开始

### 编译

API - 后端：

```bash
cargo build --release
```

frontend - 前端：

```bash
trunk build --release --cargo-profile release-wasm
```

### 配置

复制配置文件示例并根据需要修改：

```bash
cp config/ait.toml.example config/ait.toml
```

<details>
<summary>默认配置</summary>

```toml
[server]
host = "127.0.0.1"
port = 8000
health_detail = false
session_cleanup_interval_secs = 3600
rate_limiter_cleanup_interval_secs = 600
cache_cleanup_interval_secs = 300
cache_max_entries = 1000
graceful_timeout_secs = 10
trusted_proxies = ["127.0.0.1", "::1"]

[auth]
enabled = true
session_ttl_secs = 86400
bootstrap_username = "admin"
bootstrap_password = "must_replace_with_your_password"
max_api_keys_per_user = 10
rate_limiter_max_entries = 100000
login_rate_limit = { max_attempts = 5, window_secs = 300, ban_secs = 900 }

[database]
path = "./data/ait.db"

[log]
path = "./data/ait-logs.duckdb"
retention_days = 30
flush_interval_secs = 10
flush_batch = 100
channel_cap = 10000
retention_every = 100
analytics_timeout_secs = 10
level = "info"
axum = "info"
tower_http_trace = "info"

[proxy]
timeout_secs = 300
connect_timeout_secs = 30
sse_idle_timeout_secs = 60
stream = true
max_response_body_bytes = 8388608
max_request_body_bytes = 8388608

[security]
cors_allowed_origins = []
cors_allow_credentials = false
ssrf_allowed_cidrs = []

# 请求体敏感信息检测（DLP）
[security.dlp]
enabled = false
sensitive_values = []
```

**注意**：大部分配置项都可通过 `AIT_<SECTION>_<KEY>` 环境变量覆盖，例如：

```bash
export AIT_SERVER_PORT=9000
export AIT_AUTH_ENABLED=true
```

</details>

### 启动

```bash
# 使用默认配置文件 (config/ait.toml)
ait

# 指定配置文件
ait -c /path/to/config.toml
```

## Web 管理界面

构建前端后将 `frontend/dist/` 与后端二进制一起部署，启动后通过 `http://{host}:{port}` 访问。

详细说明见 [frontend/README.md](frontend/README.md)。

## OpenAI Compatible 接口

认证方式：`Authorization: Bearer <token>` ，
可以通过配置文件 `auth.enabled: false` 关闭认证，但 Admin 接口（/api/*）必须认证。

| 方法 | 路径 | 说明 |
| ------ | ------ | ------ |
| POST | /v1/chat/completions | 聊天补全 |
| POST | /v1/completions | 文本补全 |
| POST | /v1/embeddings | 文本嵌入 |
| POST | /v1/responses | 新版接口 |
| GET | /v1/models | 模型列表 |
| GET | /v1/health | 健康检查 |

## 支持的提供商类型

- [x] llama.cpp 使用 OpenAI Compatible 接口 (llama-server)
- [x] Ollama - 使用 OpenAI Compatible 接口
- [x] DeepSeek
- [x] Zhipu
- [x] OpenCode Go (OpenAI Compatible)

## 许可证

本项目采用 [MIT License](LICENSE)。

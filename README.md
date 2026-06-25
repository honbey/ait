# Ait

本项目是为了将不同大模型提供商提供的 API 统一管理，用户就可以通过一个 `base_url` 和 `api_key` 访问不同提供商提供的各种模型。

代理请求的 API 使用 OpenAI Compatible 格式。

## 特性

- **OpenAI 兼容** — 完全兼容 OpenAI API 格式，现有配置 OpenAI 接口应用只需修改  `base_url` 和 `api_key`
- **多提供商支持** — 支持 OpenAI Compatible（DeepSeek、Zhipu、LlamaCpp 等）和 Ollama
- **动态管理** — 通过 Admin API 动态添加/更新/删除提供商和模型
- **Web 管理界面** — 基于 Sycamore 0.9 的 WASM CSR 前端
- **API Key 管理** — 创建/启用/禁用/删除 API Key，支持过期时间设定和快速复制
- **用户注册** — 受控注册（可选注册码），每个用户可创建多个 API Key
- **会话认证** — HttpOnly Cookie 的登录/登出/会话管理

## 快速开始

### 编译

```bash
cargo build --release
```

### 配置

复制配置文件示例并根据需要修改：

```bash
cp config/ait.toml.example config/ait.toml
```

```toml
[server]
host = "127.0.0.1"
port = 8000
health_detail = false
session_cleanup_interval_secs = 3600

[auth]
enabled = true
session_ttl_secs = 86400
bootstrap_username = "admin"
bootstrap_password = "admin123"
allow_registration = false
registration_code = ""
max_api_keys_per_user = 10

[database]
path = "./data/ait.rocksdb"

[proxy]
timeout_secs = 300
stream = true
```

**注意**：所有配置项都可通过 `AIT_<SECTION>_<KEY>` 环境变量覆盖，例如：

```bash
export AIT_SERVER_PORT=9000
export AIT_AUTH_ENABLED=true
```

### 启动

```bash
# 使用默认配置文件 (config/ait.toml)
ait

# 指定配置文件
ait -c /path/to/config.toml
```

## Web 管理界面

编译时将前端 WASM 静态文件嵌入后端，启动后通过 `http://{host}:{port}` 访问。

目前控制台下的 dashboard/providers/models 等功能已实现，不过没有路由路径。

| 路径 | 说明 |
|------|------|
| / | 首页（其余页面正在开发中）|

## API 接口

### Proxy 接口（OpenAI 兼容）

认证方式：`Authorization: Bearer <token>` ，
可以通过配置文件 `auth.enabled: false` 关闭认证，但 Admin 接口必须认证。

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | /v1/chat/completions | 聊天补全 |
| POST | /v1/completions | 文本补全 |
| POST | /v1/embeddings | 嵌入 |
| GET  | /v1/models | 模型列表 |
| GET  | /v1/health | 健康检查 |

## 支持的提供商类型

| 类型 | 说明 |
|------|------|
| openai_compat | OpenAI 兼容接口（含 DeepSeek、Zhipu、LlamaCpp）|
| ollama | Ollama 本地模型服务 |


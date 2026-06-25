# Ait

本项目是为了将不同大模型提供商提供的 API 统一管理，
用户就可以通过一个 `base_url` 和 `api_key` 访问不同提供商提供的各种模型。

主要是代理请求到各提供商的 [OpenAI API](https://developers.openai.com/api/reference/overview) 格式的接口，
在这个过程中记录请求数、Token 用量、访问日志等方便用户审计。

## 特性

- **多提供商支持** — [llama.cpp](https://llama.app/)、[Ollama](https://ollama.com)、[DeepSeek](https://www.deepseek.com/)、[Zhipu](https://bigmodel.cn/) 等提供了 OpenAI Compatible 接口的
- **Web 管理界面** — 基于 Sycamore 0.9 的 WASM CSR 前端（ECharts 仪表盘图表）
  - **Admin 管理** — 添加/更新/删除提供商和模型
  - **API Key 管理** — 创建/启用/禁用/删除 API Key
  - **用户注册** — 受控注册（可选注册码）
- **日志审计** — 访问日志、代理请求日志（含 Token 用量）、操作审计日志，支持自动清理和每日统计

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

[auth]
enabled = true
session_ttl_secs = 86400
bootstrap_username = "admin"
bootstrap_password = "must_replace_with_your_password"
allow_registration = false
registration_code = ""
max_api_keys_per_user = 10

[database]
path = "./data/ait.rocksdb"

[log]
path = "./data/ait-logs.duckdb"
retention_days = 30
flush_interval_secs = 10
flush_batch = 100
channel_cap = 10000
retention_every = 100

[proxy]
timeout_secs = 300
stream = true
```

**注意**：所有配置项都可通过 `AIT_<SECTION>_<KEY>` 环境变量覆盖，例如：

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

编译时将前端 WASM 静态文件嵌入后端，启动后通过 `http://{host}:{port}` 访问。

详细说明见 [frontend/README.md](frontend/README.md)。

## OpenAI Compatible 接口

认证方式：`Authorization: Bearer <token>` ，
可以通过配置文件 `auth.enabled: false` 关闭认证，但 Admin 接口（/admin/*）必须认证。

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | /v1/chat/completions | 聊天补全 |
| POST | /v1/completions | 文本补全 |
| POST | /v1/embeddings | 文本嵌入 |
| GET  | /v1/models | 模型列表 |
| GET  | /v1/health | 健康检查 |

## 支持的提供商类型

- [x] llama.cpp 使用 OpenAI Compatible 接口 (llama-server)
- [x] Ollama - 使用 OpenAI Compatible 接口
- [ ] DeepSeek - 待验证
- [ ] Zhipu - 待验证

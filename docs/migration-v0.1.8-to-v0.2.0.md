# 迁移指南: v0.1.8 → v0.2.0

## 动机：为什么移除 ait 除 `/v1/*` 接口外的鉴权？

因为 ait 只是为了自己方便统计使用大模型 API 时的 token 消耗，实际用户就只有自己，
况且可以通过不同的 apikey 区分其他用户（如果有的话）。实际部署时 ait 前方还有 nginx
和 Authelia ，这两个都可以对请求鉴权再放行，ait 只需监听到 127.0.0.1 后由 nginx 反代，
配以 Authelia 的 `two_factor` 便可实现访问鉴权，如此 ait 只需专心维护 `/v1/*` 接口的逻辑，
前端相关以及管理接口可以减少代码量降低维护成本。

## 概述

v0.2.0 移除了内置的用户/会话/密码系统。管理接口 (`/api/*`) 不再由 Ait 自身鉴权，
改由外部反向代理（如 nginx + Authelia）保护。API Key 鉴权 (`/v1/*`) 保持不变。

本文档说明如何将 SQLite (`ait.db`) 和 DuckDB (`ait-logs.duckdb`) 从 v0.1.8 迁移到 v0.2.0。

## Breaking Changes

- **users / sessions 表删除** — 管理登录委托给反向代理
- **api_keys.username 列删除** — Key 不再按用户隔离，改为全局管理
- **DuckDB 日志表删除 username 列**
- **已有 API Key 失效** — 哈希算法从 bcrypt 改为 SHA256，旧 Key 无法通过鉴权，
但迁移后旧的 API Key 仍会显示在前端的列表中，但调用 `/v1/*` 会返回 401，需要手动删除。

## 前置条件

- `sqlite3` CLI
- `duckdb` CLI
- v0.1.8 数据目录备份（包含 `ait.db` 和 `ait-logs.duckdb`）
- Ait v0.2.0 二进制或源码

## 迁移步骤

### 1. 停止 Ait

确保 v0.1.8 实例完全停止后再操作数据文件。

### 2. 备份原始数据

```bash
cp -r /path/to/data /path/to/data.bak.v0.1.8
```

### 3. 迁移 SQLite (`ait.db`)

```bash
sqlite3 /path/to/data/ait.db <<'SQL'
-- 删除 v0.2.0 不再使用的表
DROP TABLE IF EXISTS users;
DROP TABLE IF EXISTS sessions;

-- 重建 api_keys，去掉 username 列
CREATE TABLE api_keys_new (
    id         TEXT PRIMARY KEY,
    key_hash   TEXT NOT NULL UNIQUE,
    display    TEXT NOT NULL,
    name       TEXT NOT NULL,
    enabled    INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    expires_at INTEGER
);

INSERT INTO api_keys_new
SELECT id, key_hash, display, name, enabled, created_at, updated_at, expires_at
FROM api_keys;

DROP TABLE api_keys;
ALTER TABLE api_keys_new RENAME TO api_keys;
CREATE INDEX idx_apikeys_id ON api_keys(id);
SQL
```

### 4. 迁移 DuckDB (`ait-logs.duckdb`)

```bash
duckdb /path/to/data/ait-logs.duckdb <<'SQL'
-- 先删除依赖索引
DROP INDEX IF EXISTS idx_access_log_timestamp;
DROP INDEX IF EXISTS idx_proxy_log_timestamp;
DROP INDEX IF EXISTS idx_audit_log_timestamp;

-- 删除 username 列
ALTER TABLE access_log DROP COLUMN username;
ALTER TABLE proxy_log DROP COLUMN username;
ALTER TABLE audit_log DROP COLUMN username;

-- 重建索引
CREATE INDEX idx_access_log_timestamp ON access_log(timestamp);
CREATE INDEX idx_proxy_log_timestamp ON proxy_log(timestamp);
CREATE INDEX idx_audit_log_timestamp ON audit_log(timestamp);
SQL
```

### 5. 更新配置文件

从 `config/ait.toml` 中删除已废弃的字段：

```toml
# 删除 [server] 中的:
# rate_limiter_cleanup_interval_secs = 600

# [auth] 仅保留 enabled 控制 /v1/* 的鉴权:
[auth]
enabled = true

# 删除以下字段:
# session_ttl_secs = 86400
# bootstrap_username = "admin"
# bootstrap_password = "admin123"
# login_rate_limit = { max_attempts = 5, window_secs = 300, ban_secs = 900 }
```

当前配置格式参见 `config/ait.toml.example`。

### 6. 部署反向代理用于管理鉴权

ait v0.2.0 不再处理管理接口认证。需要在 `/api/*` 前部署反向代理，
详见 [auth-proxy.md](auth-proxy.md) 中的 nginx + Authelia 配置示例。

### 7. 重新生成 API Key

已有 API Key 因哈希算法变更（bcrypt → SHA256）而失效。旧 Key 存储的是 bcrypt hash，v0.2.0 用 SHA256 验证，
即使发送正确的原始 key 字符串，`SHA256(原始key)` 也无法匹配旧的 bcrypt hash。

注意：**旧 Key 无法恢复，必须删除后重新生成。**

启动 ait v0.2.0 后：

1. 通过反向代理访问管理界面
2. 进入 API Keys 页面
3. 删除旧 Key，按需创建新 Key

## 验证

迁移完成后启动 Ait，验证各项功能：

```bash
# 健康检查
curl http://127.0.0.1:3000/health

# 管理接口
# 本地测试时直接访问；生产环境需通过 nginx 反代 + Authelia 鉴权
curl http://127.0.0.1:3000/api/providers
curl http://127.0.0.1:3000/api/models
curl http://127.0.0.1:3000/api/api-keys

# 代理接口（无 key 应返回 401）
curl -o /dev/null -w "%{http_code}" http://127.0.0.1:3000/v1/models
# 预期: 401
```

## Schema 对比

### SQLite `ait.db`

| 表 | v0.1.8 | v0.2.0 |
| --- | --- | --- |
| `providers` | 不变 | 不变 |
| `models` | 不变 | 不变 |
| `users` | 存在 | **删除** |
| `sessions` | 存在 | **删除** |
| `api_keys` | 有 `username` 外键 | `username` 列**删除** |

### DuckDB `ait-logs.duckdb`

| 表 | v0.1.8 | v0.2.0 |
| --- | --- | --- |
| `access_log` | 有 `username` | `username` **删除** |
| `proxy_log` | 有 `username` | `username` **删除** |
| `audit_log` | 有 `username` | `username` **删除** |

## 回滚

迁移操作基于数据文件副本，原始 `data.bak.v0.1.8/` 不受影响。如需回滚：

1. 停止 ait v0.2.0
2. 从备份恢复: `cp /path/to/data.bak.v0.1.8/* /path/to/data/`
3. 重启 ait v0.1.8

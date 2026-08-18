# 部署：nginx + Authelia 鉴权代理

Ait 本身不提供 Web 管理界面的用户/密码/2FA 功能。
生产环境下，建议在 Ait 前部署 **nginx + Authelia**，由 Authelia 负责 Web 管理面的双因素鉴权，
而 `/v1/*` 代理接口仍由 Ait 自身的 API Key 机制保护。

## 架构

```
浏览器 ──► nginx ──┬── /v1/* ──────────────► Ait (API Key 鉴权)
                   ├── /health ────────────► Ait (无鉴权)
                   └── /, /console/*, /api/*
                       │
                       ▼
                   Authelia 2FA ──► Ait (转发 Remote-User header)
```

- `/v1/*` 和 `/health`：**不经过 Authelia**，直接到达 Ait。
- 其他路径（`/`、`/console/*`、`/api/*`、`/auth/session`）：
nginx 通过 `auth_request` 指令将请求转发给 Authelia 验证，通过后在 `proxy_set_header`
中设置 `Remote-User`，Ait 据此识别管理用户身份。

## nginx 配置示例

```nginx
upstream ait_backend {
    server 127.0.0.1:8000;
}

upstream authelia_backend {
    server 127.0.0.1:9091;
}

server {
    listen 443 ssl http2;
    server_name ait.example.com;

    ssl_certificate     /etc/ssl/certs/ait.example.com.pem;
    ssl_certificate_key /etc/ssl/private/ait.example.com.key;

    # ── /v1/* : 不经过 Authelia，直接转发 ──
    location /v1/ {
        proxy_pass http://ait_backend;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_buffering off;
    }

    # ── /health : 不经过 Authelia，用于负载均衡/监控探活 ──
    location /health {
        proxy_pass http://ait_backend;
        proxy_set_header Host $host;
    }

    # ── Authelia 验证子请求（internal，外部不可直接访问）──
    location = /authelia/auth {
        internal;
        proxy_pass http://authelia_backend/auth;  # Authelia 验证端点

        proxy_pass_request_body off;
        proxy_set_header Content-Length "";
        proxy_set_header X-Original-URI     $request_uri;
        proxy_set_header X-Original-Method  $request_method;
        proxy_set_header X-Forwarded-For    $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto  $scheme;
        proxy_set_header X-Forwarded-Host   $host;
        proxy_set_header X-Forwarded-URI    $request_uri;
    }

    # ── 所有其他路径 : Authelia 2FA 鉴权 ──
    location / {
        auth_request /authelia/auth;
        error_page 401 = /authelia/login;

        # Authelia 通过 Set-Cookie header 给出会话
        auth_request_set $user    $upstream_http_remote_user;
        auth_request_set $groups  $upstream_http_remote_groups;
        auth_request_set $name    $upstream_http_remote_name;

        proxy_pass http://ait_backend;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        # Ait 读取此 header 识别管理用户身份
        proxy_set_header Remote-User       $user;
    }

    # ── Authelia 登录页面（401 时重定向）──
    location /authelia/ {
        proxy_pass http://authelia_backend/;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

> **提示**：如果 Ait 不是部署在同一台机器，将 `127.0.0.1:8000` 和 `127.0.0.1:9091` 替换为对应的实际地址。

## Authelia access_control 配置

在 Authelia 的 `configuration.yml` 中添加如下规则：

```yaml
access_control:
  default_policy: deny

  rules:
    # /v1/* 由 Ait 自身 API Key 鉴权，绕过 Authelia
    - domain: ait.example.com
      resources:
        - "^/v1(/.*)?$"
      policy: bypass

    # /health 健康检查，绕过 Authelia
    - domain: ait.example.com
      resources:
        - "^/health$"
      policy: bypass

    # 其他所有路径强制双因素鉴权
    - domain: ait.example.com
      policy: two_factor
```

## Ait 配置要点

```toml
[server]
# 如果 nginx 与 Ait 不在同一台机器，添加 nginx 所在 IP
trusted_proxies = ["127.0.0.1", "::1", "10.0.0.0/8"]

[auth]
# 控制 /v1/* 代理接口的 API Key 鉴权
# 与 nginx/Authelia 无关，始终独立生效
enabled = true

[security]
# 跨域场景（管理界面与 API 不同域名）时开启
cors_allowed_origins = ["https://ait.example.com"]
cors_allow_credentials = true
```

关键点：

- `[auth].enabled` **只影响** `/v1/*` 代理接口；`/api/*` 管理接口不再由 Ait 自身鉴权，完全依赖 nginx + Authelia。
- `trusted_proxies` 必须包含 nginx（或 CDN）的 IP，否则 `X-Forwarded-For` 不会被信任，日志中客户端 IP 会不准确。
- 如果管理界面与 API 在同一域名下（推荐），不需要配置 CORS。

## 验证清单

部署后按以下步骤验证：

| 步骤 | 命令 / 操作 | 预期结果 |
| ------ | ------------ | ---------- |
| 健康检查（无鉴权） | `curl https://ait.example.com/health` | `200 OK` |
| 代理接口无 key | `curl https://ait.example.com/v1/models` | `401 Unauthorized` |
| 代理接口有 key | `curl -H "Authorization: Bearer <key>" https://ait.example.com/v1/models` | `200 OK` + 模型列表 |
| 管理页未登录 | 浏览器访问 `https://ait.example.com/console/` | 重定向到 Authelia 登录页 |
| 管理页已登录 | 完成 2FA 后访问管理页 | 正常加载管理界面 |
| /auth/session | 登录后 `curl https://ait.example.com/auth/session` | `{"authenticated":true,"username":"<user>"}` |
| /auth/session 未登录 | `curl https://ait.example.com/auth/session` | `{"authenticated":false,"username":null}` |

## 常见问题

**Q: 关闭 `[auth].enabled` 后管理接口是否也直接暴露？**

A: 不是。`[auth].enabled` 只控制 `/v1/*`。`/api/*` 始终不经过 Ait 内置鉴权，由 nginx + Authelia 保护。
若 `auth.enabled = false`，外部未授权请求可以直接调用 `/v1/*`，但 `/api/*` 仍受 nginx 层保护。

**Q: Authelia 与 Ait 不在同一台机器可以吗？**

A: 可以。确保 nginx 的 `auth_request` 能够访问到 Authelia，且 Authelia 能够回调 Ait 进行 session 验证。

**Q: 是否可以不用 Authelia，只用 nginx 基础认证？**

A: 可以。`/auth/session` 只读取 `Remote-User` header。只要 nginx 在转发请求时设置该 header 即可。例如：

```nginx
# 简单的 HTTP Basic Auth（不推荐生产使用）
location / {
    auth_basic "Ait Admin";
    auth_basic_user_file /etc/nginx/.htpasswd;
    set $auth_user $remote_user;
    proxy_pass http://ait_backend;
    proxy_set_header Remote-User $auth_user;
}
```

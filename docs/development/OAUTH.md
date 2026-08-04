# OAuth / 本地登录

当前实现（非计划稿）。配置入口：`/config` → 用户与 OAuth。

## 能力

- **本地账号**：管理员初始化后可开 `allow_local_registration`；支持多 local 用户、改密、禁用本地登录。
- **OAuth**：统一路由 `/api/auth/oauth/:slug/*`。内置 **GitHub**；可配置多个 **OIDC** provider（Authentik / Keycloak / Google / Microsoft 等兼容方）。
- **身份表**：`user_identities` 为权威绑定源（一个外部身份对应一个用户）。登录页从 `/api/auth/oauth/providers` 动态渲染可用方式。

## 关键代码

| 区域 | 路径 |
| --- | --- |
| OAuth HTTP | `backend/src/api/oauth.rs` |
| 本地认证 | `backend/src/api/auth_local.rs` |
| Provider 实现 | `backend/src/services/oauth/`（`github`、`oidc`） |
| 配置 | `backend/src/config.rs`（`oauth_providers`） |
| 前端登录 | `frontend/src/components/LoginForm.tsx` |
| 配置 UI | `frontend/src/components/config/OAuthConfigSection.tsx` |
| 迁移 | `backend/migrations/006_oauth_identities.rs` |

## 账户匹配（登录）

1. **已绑定 identity**（`provider` + `provider_user_id`）→ 登录该用户。
2. **不再按 email 静默合并**（含 `email_verified`）：跨 issuer / 本地账号不会因邮箱相同自动接管。邮箱已被其他账号占用时，回调返回 `email_already_registered`，用户需用原方式登录后走 **link**。
3. 邮箱未占用时可创建新用户（并写入该 email）。

## CSRF / 浏览器绑定

- OAuth `state` 为 HMAC 签名 + 单次使用 nonce。
- 登录/绑定/平台授权启动时设置 HttpOnly `oauth_tx` cookie（与 state nonce 相同）；回调必须 cookie 匹配，否则 `browser_tx_mismatch`（防 login CSRF / session swapping）。
- 浏览器 API CSRF（`X-CSRF-Token`）store 仅接受**已校验**的 session JWT 签名作为 key（MYR-016）；伪造 JWT 形状不会写入 store。

## OIDC：nonce 与 PKCE（MYR-011）

- 通用 OIDC provider 在授权 URL 上附加：
  - `nonce`（与 `oauth_tx` / state `n` 相同的高熵值）
  - `code_challenge` + `code_challenge_method=S256`（PKCE）
- `code_verifier` 写入签名 state 的 `cv` 字段，任意实例可在 callback 完成 token 交换。
- `id_token` 校验要求 `nonce` claim 与签发时一致；JWKS / token 错误响应体走 limited body 读取。
- GitHub 与 Discord 数据平台 OAuth **不**走 OIDC PKCE/nonce 路径；Discord 仍使用 `oauth_tx` cookie 绑定（#262 / MYR-003）。

## 运维注意

- 生产务必配置 `JWT_SECRET`、正确的 `CORS_ORIGINS` / `BASE_URL`（回调 URL 依赖站点地址）。
- OIDC 需在 IdP 侧登记 redirect：`{BASE_URL}/api/auth/oauth/{slug}/callback`。
- GitHub 兼容字段若仍出现在旧配置里，以 `oauth_providers` 为准。

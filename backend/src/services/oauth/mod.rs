//! OAuth Provider 抽象层
//!
//! 详见 docs/development/OAUTH.md
//!
//! 通过 [`OAuthProvider`] trait 抽象不同 provider（GitHub / 通用 OIDC / ...），
//! 通用 handler 通过 [`ProviderRegistry`] 按 slug 路由。

use async_trait::async_trait;
use serde::Serialize;

pub mod github;
pub mod oidc;
pub mod registry;
pub mod state;

/// Provider 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Github,
    Oidc,
}

/// Per-login secrets for OIDC PKCE + `nonce`.
///
/// Generated at state issuance and embedded in the signed OAuth `state` so any
/// instance can complete the callback. GitHub and other non-OIDC providers
/// ignore these fields; Discord platform OAuth keeps its own flow + `oauth_tx`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthFlowSecrets {
    /// RFC 7636 code_verifier (high entropy). Used for S256 `code_challenge`.
    pub code_verifier: String,
    /// OIDC `nonce` parameter / expected `id_token` claim (same value as browser_tx).
    pub oidc_nonce: String,
}

/// 交换 code 后得到的 token 集合
///
/// 当前只用到 `access_token`（GitHub /user, OIDC userinfo）和 `id_token`（OIDC claims）。
#[derive(Debug, Clone)]
pub struct ProviderTokens {
    pub access_token: String,
    pub id_token: Option<String>,
    /// OIDC: expected `nonce` claim (set by exchange; verified in fetch_profile).
    pub expected_nonce: Option<String>,
}

/// 标准化的用户档案（跨 provider 统一格式）
#[derive(Debug, Clone)]
pub struct NormalizedProfile {
    pub provider_user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub avatar_url: Option<String>,
    pub profile_url: Option<String>,
    pub raw: serde_json::Value,
}

/// OAuth Provider trait
///
/// 每个第三方登录后端实现此 trait。三步流程：
/// 1. [`build_auth_url`](OAuthProvider::build_auth_url) — 构造授权页 URL
/// 2. [`exchange_code`](OAuthProvider::exchange_code) — 回调时用 code 换 token
/// 3. [`fetch_profile`](OAuthProvider::fetch_profile) — 用 token 拉用户档案
#[async_trait]
pub trait OAuthProvider: Send + Sync {
    /// 唯一标识，用作路由参数 `/api/auth/oauth/:slug/...`
    fn slug(&self) -> &str;

    fn kind(&self) -> ProviderKind;

    /// 给前端展示用的名字
    fn display_name(&self) -> &str;

    /// 给前端展示用的图标（URL 或内置标识，如 "github"）
    fn icon(&self) -> Option<&str>;

    /// 构造 OAuth 授权页 URL。
    ///
    /// `secrets` carries PKCE + OIDC nonce for providers that support them;
    /// non-OIDC implementations may ignore the values.
    async fn build_auth_url(
        &self,
        state: &str,
        redirect_uri: &str,
        secrets: &AuthFlowSecrets,
    ) -> Result<String, String>;

    /// 交换 code 换 token（OIDC may send `code_verifier` from `secrets`).
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        secrets: &AuthFlowSecrets,
    ) -> Result<ProviderTokens, String>;

    /// 拉取用户档案
    async fn fetch_profile(&self, tokens: &ProviderTokens) -> Result<NormalizedProfile, String>;
}

/// 给前端 `/api/auth/oauth/providers` 端点的列表项
#[derive(Debug, Clone, Serialize)]
pub struct ProviderDescriptor {
    pub slug: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub icon: Option<String>,
}

impl<T: OAuthProvider + ?Sized> From<&T> for ProviderDescriptor {
    fn from(p: &T) -> Self {
        Self {
            slug: p.slug().to_string(),
            kind: p.kind(),
            display_name: p.display_name().to_string(),
            icon: p.icon().map(|s| s.to_string()),
        }
    }
}

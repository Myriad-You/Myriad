//! GitHub OAuth provider 实现
//!
//! 从 `api/auth.rs` 抽出，遵循 [`super::OAuthProvider`] trait。

use async_trait::async_trait;
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl,
    Scope, TokenResponse, TokenUrl,
};
use serde::Deserialize;

use super::{AuthFlowSecrets, NormalizedProfile, OAuthProvider, ProviderKind, ProviderTokens};

const GITHUB_AUTH_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

#[derive(Debug, Deserialize)]
struct GitHubUser {
    id: i64,
    login: String,
    name: Option<String>,
    email: Option<String>,
    avatar_url: String,
    html_url: String,
    bio: Option<String>,
    location: Option<String>,
    company: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GithubProvider {
    client_id: String,
    client_secret: String,
}

impl GithubProvider {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret,
        }
    }
}

/// 构建一个完整端点设置的 oauth2 client。
/// 返回类型用 macro 隐藏 — `BasicClient` 的 typestate 泛型不便于显式书写。
macro_rules! build_github_client {
    ($self:expr, $redirect_uri:expr) => {{
        let auth_url = AuthUrl::new(GITHUB_AUTH_URL.to_string()).map_err(|e| e.to_string())?;
        let token_url = TokenUrl::new(GITHUB_TOKEN_URL.to_string()).map_err(|e| e.to_string())?;
        let redirect = RedirectUrl::new($redirect_uri.to_string()).map_err(|e| e.to_string())?;
        BasicClient::new(ClientId::new($self.client_id.clone()))
            .set_client_secret(ClientSecret::new($self.client_secret.clone()))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(redirect)
    }};
}

#[async_trait]
impl OAuthProvider for GithubProvider {
    fn slug(&self) -> &str {
        "github"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Github
    }

    fn display_name(&self) -> &str {
        "GitHub"
    }

    fn icon(&self) -> Option<&str> {
        Some("github")
    }

    async fn build_auth_url(
        &self,
        state: &str,
        redirect_uri: &str,
        _secrets: &AuthFlowSecrets,
    ) -> Result<String, String> {
        // GitHub is plain OAuth2 (no OIDC nonce / no PKCE required). Secrets are
        // ignored so the shared login/link handlers stay provider-agnostic.
        let client = build_github_client!(self, redirect_uri);
        let (url, _csrf) = client
            .authorize_url(|| CsrfToken::new(state.to_string()))
            .add_scope(Scope::new("read:user".to_string()))
            .add_scope(Scope::new("user:email".to_string()))
            .url();
        Ok(url.to_string())
    }

    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        _secrets: &AuthFlowSecrets,
    ) -> Result<ProviderTokens, String> {
        let client = build_github_client!(self, redirect_uri);
        // GITHUB_TOKEN_URL 是硬编码常量，这里没有 SSRF 面；用安全客户端是为了
        // 拿到超时与禁重定向 —— 裸 `reqwest::Client::new()` 没有超时，
        // 对端挂起时这次交换会无限期占住请求。
        let (_endpoint, http) = crate::services::outbound_security::build_public_http_client(
            GITHUB_TOKEN_URL,
            std::time::Duration::from_secs(15),
            Some("Myriad-App"),
        )
        .await
        .map_err(|e| format!("GitHub token endpoint rejected by outbound policy: {e}"))?;
        let token = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .request_async(&http)
            .await
            .map_err(|e| format!("GitHub token exchange failed: {e:?}"))?;

        Ok(ProviderTokens {
            access_token: token.access_token().secret().clone(),
            id_token: None,
            expected_nonce: None,
        })
    }

    async fn fetch_profile(&self, tokens: &ProviderTokens) -> Result<NormalizedProfile, String> {
        let user_url = format!(
            "{}/user",
            crate::services::http_client::GitHubApiUrl::get_api_base().await
        );

        // API base 可由管理员配置（GitHub Enterprise），所以这条出站也走
        // SSRF 策略：解析后校验公网可路由、pin 住 DNS、禁用重定向。
        // 风险低于 OIDC（那里下一跳由远端 provider 决定），但成本同样低。
        let (endpoint, http) = crate::services::outbound_security::build_public_http_client(
            &user_url,
            std::time::Duration::from_secs(15),
            Some("Myriad-App"),
        )
        .await
        .map_err(|e| format!("GitHub API base rejected by outbound policy: {e}"))?;

        let resp = http
            .get(endpoint)
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .send()
            .await
            .map_err(|e| format!("GitHub /user request failed: {e:?}"))?;
        let bytes = crate::services::outbound_security::read_limited_body(resp, 512 * 1024)
            .await
            .map_err(|e| format!("GitHub /user body rejected: {e}"))?;
        let user: GitHubUser = serde_json::from_slice(&bytes)
            .map_err(|e| format!("GitHub /user parse failed: {e:?}"))?;

        let raw = serde_json::json!({
            "id": user.id,
            "login": user.login,
            "name": user.name,
            "email": user.email,
            "avatar_url": user.avatar_url,
            "html_url": user.html_url,
            "bio": user.bio,
            "location": user.location,
            "company": user.company,
        });

        Ok(NormalizedProfile {
            provider_user_id: user.id.to_string(),
            username: user.login,
            email: user.email,
            email_verified: false, // GitHub /user 不返回 verified 标记；需走 /user/emails 才能判断
            avatar_url: Some(user.avatar_url),
            profile_url: Some(user.html_url),
            raw,
        })
    }
}

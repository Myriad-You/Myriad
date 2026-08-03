//! 通用 OIDC Provider 实现
//!
//! 设计：
//! - **Discovery**: 启动时从 `discovery_url` 拉 `.well-known/openid-configuration`，
//! 缓存 24h（lazy 刷新）。
//! - **Token 交换**: 标准 OAuth2 `authorization_code` flow，POST 到 `token_endpoint`。
//! - **Profile**: 优先解析 `id_token` 的 claims；缺失字段再去 `userinfo_endpoint` 拉。
//! - **id_token 验证**: 通过 discovery 的 `jwks_uri` 拉取 JWKS，校验签名、
//! `iss`、`aud`、`exp`、`sub` 和 `azp`。
//!
//! 详见 docs/development/OAUTH.md

use async_trait::async_trait;
use jsonwebtoken::{
    decode, decode_header,
    jwk::{AlgorithmParameters, Jwk, JwkSet, PublicKeyUse},
    Algorithm, DecodingKey, Validation,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::{NormalizedProfile, OAuthProvider, ProviderKind, ProviderTokens};

const DISCOVERY_TTL: Duration = Duration::from_secs(24 * 3600);

/// OIDC 出站请求的超时与响应体上限。
///
/// discovery / JWKS / userinfo / token 都是小 JSON；给足余量即可。
const OIDC_HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const OIDC_MAX_BODY: usize = 512 * 1024;

/// 为一个 OIDC 端点构造受 SSRF 约束的客户端。
///
/// # 为什么必须走这里
///
/// `discovery_url` 由管理员配置（半可信），但 **`jwks_uri` / `token_endpoint` /
/// `userinfo_endpoint` 来自 discovery 响应本身** —— 也就是说远端 provider 决定
/// 我们下一跳连到哪里。一个恶意或被攻陷的 provider 可以把它们指向
/// `http://169.254.169.254/` 或内网地址，用我们的后端当跳板。
///
/// [`build_public_http_client`] 会解析域名、逐个校验解析出的地址公网可路由、
/// 把 DNS 结果 pin 住并禁用重定向，堵死 DNS 重绑定与重定向两条路。
///
/// **行为变化**：禁用重定向。OIDC 端点通常不重定向；若某个 provider 依赖
/// 重定向，需要逐跳校验后为每跳单独建客户端，而不是放开这里。
async fn oidc_client(url: &str) -> Result<(url::Url, reqwest::Client), String> {
    crate::services::outbound_security::build_public_http_client(
        url,
        OIDC_HTTP_TIMEOUT,
        Some("Myriad-OIDC"),
    )
    .await
    .map_err(|e| format!("OIDC endpoint rejected by outbound policy ({url}): {e}"))
}

/// 读取受限长度的响应体并按 JSON 解析。
async fn oidc_json<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    what: &str,
) -> Result<T, String> {
    let bytes = crate::services::outbound_security::read_limited_body(resp, OIDC_MAX_BODY)
        .await
        .map_err(|e| format!("OIDC {what} body rejected: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("OIDC {what} JSON parse failed: {e:?}"))
}

/// OIDC discovery 文档（只保留我们用得到的字段）
#[derive(Debug, Clone, Deserialize)]
struct DiscoveryDoc {
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    userinfo_endpoint: Option<String>,
    #[serde(default)]
    jwks_uri: Option<String>,
    #[serde(default)]
    issuer: Option<String>,
}

#[derive(Debug)]
struct DiscoveryCache {
    doc: DiscoveryDoc,
    fetched_at: Instant,
}

pub struct OidcProvider {
    slug: String,
    display_name: String,
    icon: Option<String>,
    client_id: String,
    client_secret: String,
    scopes: Vec<String>,
    discovery_url: String,
    cache: Arc<RwLock<Option<DiscoveryCache>>>,
}

impl OidcProvider {
    pub fn new(
        slug: String,
        display_name: String,
        icon: Option<String>,
        client_id: String,
        client_secret: String,
        scopes: Vec<String>,
        discovery_url: String,
    ) -> Self {
        Self {
            slug,
            display_name,
            icon,
            client_id,
            client_secret,
            scopes,
            discovery_url,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    async fn discovery(&self) -> Result<DiscoveryDoc, String> {
        {
            let guard = self.cache.read().await;
            if let Some(c) = guard.as_ref() {
                if c.fetched_at.elapsed() < DISCOVERY_TTL {
                    return Ok(c.doc.clone());
                }
            }
        }

        tracing::debug!("🔄 Refreshing OIDC discovery for {}", self.slug);
        let (endpoint, http) = oidc_client(&self.discovery_url).await?;
        let resp = http
            .get(endpoint)
            .send()
            .await
            .map_err(|e| format!("OIDC discovery GET failed: {e:?}"))?;
        let doc: DiscoveryDoc = oidc_json(resp, "discovery").await?;

        let mut guard = self.cache.write().await;
        *guard = Some(DiscoveryCache {
            doc: doc.clone(),
            fetched_at: Instant::now(),
        });
        Ok(doc)
    }

    fn scope_string(&self) -> String {
        if self.scopes.is_empty() {
            "openid email profile".to_string()
        } else if self.scopes.iter().any(|s| s == "openid") {
            self.scopes.join(" ")
        } else {
            // 必须包含 openid
            let mut s = vec!["openid".to_string()];
            s.extend(self.scopes.iter().cloned());
            s.join(" ")
        }
    }

    async fn fetch_jwks(&self, doc: &DiscoveryDoc) -> Result<JwkSet, String> {
        let jwks_uri = doc
            .jwks_uri
            .as_deref()
            .ok_or_else(|| "OIDC discovery missing 'jwks_uri'".to_string())?;
        // jwks_uri 来自 discovery 响应 —— 由远端决定，必须走 SSRF 策略
        let (endpoint, http) = oidc_client(jwks_uri).await?;
        let resp = http
            .get(endpoint)
            .send()
            .await
            .map_err(|e| format!("OIDC JWKS GET failed: {e:?}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("OIDC JWKS endpoint returned {status}: {body}"));
        }

        resp.json::<JwkSet>()
            .await
            .map_err(|e| format!("OIDC JWKS JSON parse failed: {e:?}"))
    }

    async fn verify_id_token(&self, id_token: &str) -> Result<serde_json::Value, String> {
        let doc = self.discovery().await?;
        let issuer = doc
            .issuer
            .as_deref()
            .ok_or_else(|| "OIDC discovery missing 'issuer'".to_string())?;
        let header =
            decode_header(id_token).map_err(|e| format!("OIDC id_token header invalid: {e}"))?;

        ensure_asymmetric_id_token_alg(header.alg)?;

        let jwks = self.fetch_jwks(&doc).await?;
        let jwk = select_jwk(&jwks, header.kid.as_deref())?;
        ensure_jwk_matches_id_token(jwk, header.alg)?;

        let key = DecodingKey::from_jwk(jwk)
            .map_err(|e| format!("OIDC JWK decoding key invalid: {e}"))?;
        let mut validation = Validation::new(header.alg);
        validation.set_audience(&[self.client_id.as_str()]);
        validation.set_issuer(&[issuer]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.validate_nbf = true;

        let data = decode::<serde_json::Value>(id_token, &key, &validation)
            .map_err(|e| format!("OIDC id_token verification failed: {e}"))?;
        validate_authorized_party(&data.claims, &self.client_id)?;
        Ok(data.claims)
    }
}

#[async_trait]
impl OAuthProvider for OidcProvider {
    fn slug(&self) -> &str {
        &self.slug
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Oidc
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn icon(&self) -> Option<&str> {
        self.icon.as_deref()
    }

    async fn build_auth_url(&self, state: &str, redirect_uri: &str) -> Result<String, String> {
        let doc = self.discovery().await?;
        let scope = self.scope_string();
        // 用 url crate 解析 + append query，避免 authorization_endpoint 本身带 ?param 时
        // 拼出 https://x?a=b?response_type=code 这种非法 URL
        let mut url = url::Url::parse(&doc.authorization_endpoint)
            .map_err(|e| format!("invalid authorization_endpoint: {e}"))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", &scope)
            .append_pair("state", state);
        // TODO(MYR-011): add PKCE S256 (code_challenge / code_verifier) and OIDC
        // `nonce` once the OAuthProvider trait can carry per-login secrets without
        // breaking GitHub / multi-instance state. Browser CSRF is already bound via
        // oauth_tx + signed state (MYR-003).
        Ok(url.into())
    }

    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<ProviderTokens, String> {
        let doc = self.discovery().await?;
        // token_endpoint 同样来自 discovery 响应
        let (endpoint, http) = oidc_client(&doc.token_endpoint).await?;

        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
        ];

        #[derive(Debug, Deserialize)]
        struct TokenResp {
            access_token: String,
            #[serde(default)]
            id_token: Option<String>,
        }

        let resp = http
            .post(endpoint)
            .header("Accept", "application/json")
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("OIDC token POST failed: {e:?}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("OIDC token endpoint returned {status}: {body}"));
        }

        let token: TokenResp = oidc_json(resp, "token").await?;

        Ok(ProviderTokens {
            access_token: token.access_token,
            id_token: token.id_token,
        })
    }

    async fn fetch_profile(&self, tokens: &ProviderTokens) -> Result<NormalizedProfile, String> {
        let id_token = tokens
            .id_token
            .as_deref()
            .ok_or_else(|| "OIDC token response missing 'id_token'".to_string())?;
        let id_claims = self.verify_id_token(id_token).await?;

        // 如果 id_token 没给齐档案，再去 userinfo
        let userinfo: serde_json::Value = match id_claims.get("email") {
            Some(_) => id_claims.clone(),
            _ => {
                let doc = self.discovery().await?;
                if let Some(url) = doc.userinfo_endpoint.as_ref() {
                    // userinfo_endpoint 也来自 discovery 响应
                    let (endpoint, http) = oidc_client(url).await?;
                    let resp = http
                        .get(endpoint)
                        .bearer_auth(&tokens.access_token)
                        .send()
                        .await
                        .map_err(|e| format!("OIDC userinfo GET failed: {e:?}"))?;
                    oidc_json::<serde_json::Value>(resp, "userinfo").await?
                } else {
                    id_claims.clone()
                }
            }
        };

        if let (Some(id_sub), Some(userinfo_sub)) = (
            id_claims.get("sub").and_then(|v| v.as_str()),
            userinfo.get("sub").and_then(|v| v.as_str()),
        ) {
            if id_sub != userinfo_sub {
                return Err("OIDC userinfo 'sub' does not match id_token".to_string());
            }
        }

        let sub = userinfo
            .get("sub")
            .and_then(|v| v.as_str())
            .or_else(|| id_claims.get("sub").and_then(|v| v.as_str()))
            .ok_or_else(|| "OIDC profile missing 'sub'".to_string())?
            .to_string();

        let preferred_username = userinfo
            .get("preferred_username")
            .and_then(|v| v.as_str())
            .or_else(|| userinfo.get("name").and_then(|v| v.as_str()))
            .or_else(|| userinfo.get("email").and_then(|v| v.as_str()))
            .or_else(|| id_claims.get("preferred_username").and_then(|v| v.as_str()))
            .or_else(|| id_claims.get("name").and_then(|v| v.as_str()))
            .or_else(|| id_claims.get("email").and_then(|v| v.as_str()))
            .unwrap_or(&sub)
            .to_string();

        let email = userinfo
            .get("email")
            .and_then(|v| v.as_str())
            .or_else(|| id_claims.get("email").and_then(|v| v.as_str()))
            .map(|s| s.to_string());

        // email_verified: 优先 userinfo 显式声明，缺省 false（保守 — 不会触发自动 merge）
        let email_verified = userinfo
            .get("email_verified")
            .and_then(|v| v.as_bool())
            .or_else(|| id_claims.get("email_verified").and_then(|v| v.as_bool()))
            .unwrap_or(false);

        let avatar_url = userinfo
            .get("picture")
            .and_then(|v| v.as_str())
            .or_else(|| id_claims.get("picture").and_then(|v| v.as_str()))
            .map(|s| s.to_string());

        let profile_url = userinfo
            .get("profile")
            .and_then(|v| v.as_str())
            .or_else(|| id_claims.get("profile").and_then(|v| v.as_str()))
            .map(|s| s.to_string());

        Ok(NormalizedProfile {
            provider_user_id: sub,
            username: preferred_username,
            email,
            email_verified,
            avatar_url,
            profile_url,
            raw: userinfo,
        })
    }
}

fn ensure_asymmetric_id_token_alg(alg: Algorithm) -> Result<(), String> {
    match alg {
        Algorithm::RS256
        | Algorithm::RS384
        | Algorithm::RS512
        | Algorithm::PS256
        | Algorithm::PS384
        | Algorithm::PS512
        | Algorithm::ES256
        | Algorithm::ES384
        | Algorithm::EdDSA => Ok(()),
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => Err(format!(
            "OIDC id_token algorithm {:?} is not accepted; configure provider for asymmetric signing",
            alg
        )),
    }
}

fn select_jwk<'a>(jwks: &'a JwkSet, kid: Option<&str>) -> Result<&'a Jwk, String> {
    if let Some(kid) = kid {
        return jwks
            .find(kid)
            .ok_or_else(|| format!("OIDC JWKS does not contain kid '{kid}'"));
    }

    let mut candidates = jwks
        .keys
        .iter()
        .filter(|jwk| !matches!(&jwk.algorithm, AlgorithmParameters::OctetKey(_)));
    match (candidates.next(), candidates.next()) {
        (Some(jwk), None) => Ok(jwk),
        (None, _) => Err("OIDC id_token missing 'kid' and JWKS has no asymmetric keys".to_string()),
        (Some(_), Some(_)) => {
            Err("OIDC id_token missing 'kid' and JWKS has multiple keys".to_string())
        }
    }
}

fn ensure_jwk_matches_id_token(jwk: &Jwk, alg: Algorithm) -> Result<(), String> {
    if let Some(key_use) = jwk.common.public_key_use.as_ref() {
        if key_use != &PublicKeyUse::Signature {
            return Err("OIDC JWK is not marked for signature use".to_string());
        }
    }

    if matches!(&jwk.algorithm, AlgorithmParameters::OctetKey(_)) {
        return Err("OIDC JWK uses a symmetric key, which is not accepted".to_string());
    }

    if let Some(key_alg) = jwk.common.key_algorithm.as_ref() {
        let key_alg = key_alg.to_string();
        let token_alg = format!("{:?}", alg);
        if key_alg != token_alg {
            return Err(format!(
                "OIDC JWK alg '{key_alg}' does not match id_token alg '{token_alg}'"
            ));
        }
    }

    Ok(())
}

fn validate_authorized_party(claims: &serde_json::Value, client_id: &str) -> Result<(), String> {
    let azp = claims.get("azp").and_then(|v| v.as_str());
    if let Some(azp) = azp {
        if azp != client_id {
            return Err("OIDC id_token 'azp' does not match client_id".to_string());
        }
    }

    let aud_count = match claims.get("aud") {
        Some(serde_json::Value::Array(values)) => values.len(),
        Some(_) => 1,
        None => 0,
    };
    if aud_count > 1 && azp.is_none() {
        return Err("OIDC id_token has multiple audiences but no 'azp'".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod outbound_policy_tests {
    /// OAuth 的每一条出站请求都必须经过 `outbound_security`。
    ///
    /// # 为什么用源码断言
    ///
    /// OIDC 的 `jwks_uri` / `token_endpoint` / `userinfo_endpoint` **来自 discovery
    /// 响应本身** —— 远端 provider 决定我们下一跳连哪里。任何一处退回裸
    /// `reqwest::Client` 或全局客户端，就重新打开一条「provider 指哪我们打哪」
    /// 的 SSRF 通道，而且不会有任何编译期信号。
    ///
    /// 类型系统表达不了「这个 Client 是受策略约束的那个」，所以对源码断言。
    #[test]
    fn oauth_modules_never_use_an_unrestricted_http_client() {
        for (name, src) in [
            ("oidc.rs", include_str!("oidc.rs")),
            ("github.rs", include_str!("github.rs")),
        ] {
            // 去掉测试模块自身与所有注释：断言的是**代码**，不是文档里
            // 提到的字面量（否则解释"为什么不用裸客户端"的注释会自己触发）。
            let code: String = src
                .split("#[cfg(test)]")
                .next()
                .unwrap_or(src)
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            let code = code.as_str();

            assert!(
                !code.contains("get_global_client"),
                "{name}: the shared client has no SSRF policy, DNS pinning, or body \
                 limit — route OAuth traffic through outbound_security instead"
            );
            assert!(
                !code.contains("reqwest::Client::new()"),
                "{name}: a bare reqwest client follows redirects and buffers unbounded \
                 bodies — route it through outbound_security::build_public_http_client"
            );
        }
    }
}

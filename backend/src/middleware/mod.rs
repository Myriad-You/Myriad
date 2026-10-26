pub mod auth;
pub mod client_ip;
pub mod cookie_secure; // 局域网 http 访问时去掉 cookie 的 Secure
pub mod cors_runtime;
pub mod csrf; // CSRF 防护中间件
pub mod federation_gate; // Egress-location kill switch for the federation surface
pub mod rate_limit;
pub mod security;
pub mod sync_device; // Device tokens for syncing her (`/api/agent/sync/*`)
pub mod ws_origin;

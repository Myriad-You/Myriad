//! Data encryption key for Myriad configuration secrets and federation private-key envelopes.
//!
//! Workspace crate so `federation` and `services` share one AES-GCM keying path without
//! layering through the HTTP `api` tree. Ciphertext format: `myriad-enc:v1:<nonce>:<ct>`.
//!
//! DB migration of legacy plaintext rows stays in the backend (SeaORM); pure crypto lives here.

//! 数据加密密钥（配置密钥 / 联邦私钥的信封密钥）
//!
//! # 背景
//!
//! 修复前有两个问题：
//!
//! 1. `configurations.is_encrypted` 只是一个布尔列。它按 key 名把 `*_api_key` /
//!    `*_token` / `*_secret` 标成"已加密"，但全仓没有任何加解密代码，值就是明文
//!    落库。任何拿到数据库副本的路径（`pg_dump`、面板的数据库管理、只读副本、
//!    updater 的 pgdata 快照）都能直接读到全部平台密钥与 OAuth secret。
//! 2. 联邦私钥确实加密了，但 AES 密钥派生自 `JWT_SECRET`。于是轮换 `JWT_SECRET`
//!    （即 session 密钥泄露后的标准处置）会让历史私钥永久无法解密 —— 实例丢掉
//!    ActivityPub 身份，全部既有关注关系作废。结果就是没人敢轮换。
//!
//! # 密钥来源
//!
//! 按顺序解析，先命中者生效：
//!
//! 1. `MYRIAD_DATA_KEY` 环境变量（base64 的 32 字节）—— 给希望用外部机制托管
//!    密钥的部署。
//! 2. `{DATA_DIR}/.secret-key` 文件 —— **默认路径**。不存在时自动生成，权限 0600。
//! 3. 兜底：从 `JWT_SECRET` 派生，并打 warn。
//!
//! 选文件而不是环境变量，对已有部署有三个具体好处：
//!
//! - `DATA_DIR` 已经是挂载好的卷（compose 里的 `backend_data`），升级**不需要**
//!   改 compose、改 `.env` 或任何手工步骤，也不会被 updater 的自动升级绕过。
//! - 它和数据库不在同一个备份域：PostgreSQL 走 `./pgdata` bind mount，
//!   `backend_data` 是具名卷。所有泄露数据库的路径都拿不到这个密钥文件 ——
//!   这正是加密方案成立的前提。
//! - 它不进程序的环境变量表。MCP 子进程会继承后端的整个环境
//!   （见 `services/agent/mcp/transport.rs`），密钥放 env 等于发给每个 MCP server。
//!
//! 兜底分支保证**已有部署升级后不会启动失败**：卷只读、权限异常等情况下退回到
//! 旧行为并告警，而不是拒绝启动。
//!
//! # 密文格式
//!
//! `myriad-enc:v1:<base64(nonce)>:<base64(ciphertext)>`
//!
//! 前缀带版本号，所以将来轮换密钥时可以新旧并存、惰性重加密，不会重演
//! 「JWT_SECRET 一换全盘解不开」的死局。前缀足够长，不会和真实配置值撞上。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use aes_gcm::{aead::Aead, Aes256Gcm, Key, KeyInit, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use sha2::{Digest, Sha256};

const AES_NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// 密文前缀。改动它等于换格式，必须同时保留旧前缀的解密路径。
pub const CIPHERTEXT_PREFIX: &str = "myriad-enc:v1:";

/// 密钥文件名（位于 `DATA_DIR` 下）
const KEY_FILE_NAME: &str = ".secret-key";

/// 密钥来源，仅用于日志与诊断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// `MYRIAD_DATA_KEY` 环境变量
    Env,
    /// `{DATA_DIR}/.secret-key`（已存在）
    File,
    /// `{DATA_DIR}/.secret-key`（本次启动新建）
    Generated,
    /// 兜底：从 JWT_SECRET 派生
    LegacyJwtSecret,
}

impl KeySource {
    pub fn as_str(self) -> &'static str {
        match self {
            KeySource::Env => "env:MYRIAD_DATA_KEY",
            KeySource::File => "file",
            KeySource::Generated => "file(generated)",
            KeySource::LegacyJwtSecret => "legacy:JWT_SECRET",
        }
    }

    /// 兜底来源意味着 2a/2b 的加固**没有真正生效**，值得持续告警。
    pub fn is_fallback(self) -> bool {
        matches!(self, KeySource::LegacyJwtSecret)
    }
}

pub struct DataKey {
    key: [u8; KEY_LEN],
    source: KeySource,
}

static DATA_KEY: OnceLock<DataKey> = OnceLock::new();

impl DataKey {
    pub fn source(&self) -> KeySource {
        self.source
    }

    /// 密钥指纹（SHA-256 前 8 个 hex 字符）。
    ///
    /// 用于在日志里确认"换机后用的还是同一把钥匙"，本身不泄露密钥。
    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"myriad-data-key-fingerprint:");
        hasher.update(self.key);
        hex::encode(&hasher.finalize()[..4])
    }

    /// Build AES-256-GCM from the fixed 32-byte data key.
    ///
    /// Explicit `Key::<Aes256Gcm>::from_slice` keeps fixed-size keying clear and
    /// avoids fallible `new_from_slice` plus `anyhow::Context` (needs
    /// crypto-common 0.1 `std`, no longer pulled in via sha2 0.11).
    /// `from_slice` is deprecated on generic-array 0.14; still the aes-gcm 0.10 path.
    #[allow(deprecated)]
    fn aes_cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key))
    }

    /// 加密为 `myriad-enc:v1:<nonce>:<ct>`
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let cipher = self.aes_cipher();
        let nonce_bytes = rand::random::<[u8; AES_NONCE_LEN]>();
        let ciphertext = cipher
            .encrypt(&Nonce::from(nonce_bytes), plaintext.as_bytes())
            .map_err(|e| anyhow!("AES-GCM encryption failed: {e}"))?;
        Ok(format!(
            "{}{}:{}",
            CIPHERTEXT_PREFIX,
            BASE64.encode(nonce_bytes),
            BASE64.encode(&ciphertext)
        ))
    }

    /// 解密 `myriad-enc:v1:` 密文。
    ///
    /// 传入的不是本格式密文时返回 `Err` —— 调用方据此判断"这是遗留明文"。
    pub fn decrypt(&self, stored: &str) -> Result<String> {
        let rest = stored
            .strip_prefix(CIPHERTEXT_PREFIX)
            .ok_or_else(|| anyhow!("not a {CIPHERTEXT_PREFIX} ciphertext"))?;
        let (nonce_b64, ct_b64) = rest
            .split_once(':')
            .ok_or_else(|| anyhow!("malformed ciphertext: missing nonce separator"))?;

        let nonce_bytes: [u8; AES_NONCE_LEN] = BASE64
            .decode(nonce_b64)
            .context("Failed to decode nonce")?
            .try_into()
            .map_err(|_| anyhow!("Invalid nonce length"))?;
        let ciphertext = BASE64
            .decode(ct_b64)
            .context("Failed to decode ciphertext")?;

        let cipher = self.aes_cipher();
        let plaintext = cipher
            .decrypt(&Nonce::from(nonce_bytes), ciphertext.as_ref())
            .map_err(|e| anyhow!("AES-GCM decryption failed: {e}"))?;
        String::from_utf8(plaintext).context("Decrypted value is not valid UTF-8")
    }
}

/// 某个存储值是否是本模块产出的密文。
pub fn is_ciphertext(stored: &str) -> bool {
    stored.starts_with(CIPHERTEXT_PREFIX)
}

/// 密钥文件路径：`{DATA_DIR}/.secret-key`
fn key_file_path() -> PathBuf {
    let root = std::env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string());
    Path::new(&root).join(KEY_FILE_NAME)
}

fn parse_key_material(raw: &str) -> Option<[u8; KEY_LEN]> {
    let bytes = BASE64.decode(raw.trim()).ok()?;
    bytes.try_into().ok()
}

fn write_key_file(path: &Path, encoded: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // create_new：并发启动时只有一个进程能成功建文件，另一个回去读它，
    // 避免两个进程各自生成密钥、后写的那把覆盖先写的。
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    use std::io::Write;
    let mut file = options.open(path)?;
    writeln!(file, "{encoded}")?;
    file.sync_all()
}

fn load_or_create() -> DataKey {
    // 1) 环境变量覆盖
    if let Ok(raw) = std::env::var("MYRIAD_DATA_KEY") {
        if !raw.trim().is_empty() {
            match parse_key_material(&raw) {
                Some(key) => {
                    return DataKey {
                        key,
                        source: KeySource::Env,
                    }
                }
                None => tracing::error!(
                    "MYRIAD_DATA_KEY is set but is not base64-encoded 32 bytes; ignoring it"
                ),
            }
        }
    }

    // 2) 密钥文件
    let path = key_file_path();
    if let Ok(contents) = std::fs::read_to_string(&path) {
        if let Some(key) = parse_key_material(&contents) {
            return DataKey {
                key,
                source: KeySource::File,
            };
        }
        // 文件存在但内容不可用：绝不覆盖它 —— 覆盖等于把已有密文全部变成垃圾。
        tracing::error!(
            path = %path.display(),
            "Data key file exists but is unreadable as base64 32 bytes; refusing to overwrite it"
        );
    } else {
        // 3) 自动生成
        let key = rand::random::<[u8; KEY_LEN]>();
        let encoded = BASE64.encode(key);
        match write_key_file(&path, &encoded) {
            Ok(()) => {
                tracing::info!(path = %path.display(), "🔑 Generated a new data encryption key");
                return DataKey {
                    key,
                    source: KeySource::Generated,
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // 与另一个启动中的进程赛跑输了 —— 读它写的那把。
                if let Some(key) = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|c| parse_key_material(&c))
                {
                    return DataKey {
                        key,
                        source: KeySource::File,
                    };
                }
            }
            Err(e) => tracing::error!(
                path = %path.display(),
                "Failed to write data key file: {e}"
            ),
        }
    }

    // 4) 兜底：从 JWT_SECRET 派生。
    //
    // 保证已有部署在卷只读/权限异常时仍能启动，代价是这次启动没有真正解耦。
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_default();
    tracing::warn!(
        path = %path.display(),
        "⚠️  Falling back to a JWT_SECRET-derived data key. Secrets remain tied to \
         JWT_SECRET, so rotating it will make stored federation keys undecryptable. \
         Fix the data directory permissions, or set MYRIAD_DATA_KEY."
    );
    DataKey {
        key: derive_legacy_key(&jwt_secret),
        source: KeySource::LegacyJwtSecret,
    }
}

/// 遗留派生：`SHA-256("myriad-federation-key-encryption:" || jwt_secret)`
///
/// 必须与 `federation::keys` 的历史实现逐字节一致，否则升级后既有联邦私钥
/// 解不开。它同时是 v0 密文的解密密钥。
pub fn derive_legacy_key(jwt_secret: &str) -> [u8; KEY_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(b"myriad-federation-key-encryption:");
    hasher.update(jwt_secret.as_bytes());
    hasher.finalize().into()
}

/// 进程级数据密钥（首次调用时解析）。
pub fn data_key() -> &'static DataKey {
    DATA_KEY.get_or_init(load_or_create)
}

// ==================== configurations 表的封装 ====================

/// 某个配置 key 的值是否属于敏感数据。
///
/// 这条规则同时决定 `configurations.is_encrypted` 列的取值 —— 在加密落地之前，
/// 那个列只是个标签，值照样明文入库。现在它和实际加密行为绑定了。
///
/// # 匹配策略
///
/// 用子串会误伤配额 / 元数据字段（`user_ai_daily_tokens`、`openai_max_tokens`、
/// `discord_token_expires_at`）。对这些**明确非密钥**的形状先排除，再匹配
/// `api_key` / `token` / `secret` / `password` / `npsso`。
pub fn is_sensitive_config_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();

    // 配额、上限、过期时间戳 —— 名字里带 token 但不是密钥。
    if key.ends_with("_tokens")
        || key.contains("max_tokens")
        || key.ends_with("_expires_at")
        || key.contains("token_expires")
    {
        return false;
    }

    key.contains("api_key")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("npsso")
        || key.contains("certificate")
}

/// 写库前封装：敏感 key 的字符串值加密，其余原样返回。
///
/// 只处理 JSON 字符串。敏感配置都是字符串形态；对象/数组不做部分加密，
/// 免得产生"一半密文一半明文"的半吊子状态。
pub fn seal_config_value(key: &str, value: serde_json::Value) -> serde_json::Value {
    if !is_sensitive_config_key(key) {
        return value;
    }
    let serde_json::Value::String(ref plain) = value else {
        return value;
    };
    // 空值不加密：空字符串是"未配置"的语义，加密它只会让 UI 误以为已设置。
    if plain.is_empty() || is_ciphertext(plain) {
        return value;
    }
    match data_key().encrypt(plain) {
        Ok(ct) => serde_json::Value::String(ct),
        Err(e) => {
            // 加密失败时宁可不写密文，也不能把值弄丢。
            tracing::error!(key, "Failed to encrypt configuration value: {e}");
            value
        }
    }
}

/// 读库后解封：识别到密文就解密，否则原样返回（遗留明文）。
pub fn open_config_value(key: &str, value: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::String(ref stored) = value else {
        return value;
    };
    if !is_ciphertext(stored) {
        return value; // 尚未迁移的遗留明文
    }
    match data_key().decrypt(stored) {
        Ok(plain) => serde_json::Value::String(plain),
        Err(e) => {
            // 解不开通常意味着换了密钥。返回空串而不是密文本身 ——
            // 否则密文会被当成 API key 发给上游，产生莫名其妙的报错。
            tracing::error!(key, "Failed to decrypt configuration value: {e}");
            serde_json::Value::String(String::new())
        }
    }
}

/// 启动时调用一次，把密钥来源写进日志。
pub fn log_startup_state() {
    let key = data_key();
    if key.source().is_fallback() {
        tracing::warn!(
            source = key.source().as_str(),
            fingerprint = %key.fingerprint(),
            "Data key active (fallback mode — secrets still tied to JWT_SECRET)"
        );
    } else {
        tracing::info!(
            source = key.source().as_str(),
            fingerprint = %key.fingerprint(),
            "🔐 Data key active"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(byte: u8) -> DataKey {
        DataKey {
            key: [byte; KEY_LEN],
            source: KeySource::File,
        }
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        // aes-gcm 0.11: Aes256Gcm::new_from_slice + Nonce::from([u8;12]) AEAD path
        // used for config secrets and federation private-key envelopes.
        let k = test_key(7);
        for value in ["", "sk-abc123", "带中文的值", "with:colons:and=equals"] {
            let ct = k.encrypt(value).unwrap();
            assert!(is_ciphertext(&ct), "{ct}");
            assert_eq!(k.decrypt(&ct).unwrap(), value);
            assert!(ct.starts_with(CIPHERTEXT_PREFIX));
            // Wire format: myriad-enc:v1:<b64-nonce>:<b64-ct>
            let rest = ct.strip_prefix(CIPHERTEXT_PREFIX).unwrap();
            let (nonce_b64, _) = rest.split_once(':').unwrap();
            let nonce = BASE64.decode(nonce_b64).unwrap();
            assert_eq!(nonce.len(), AES_NONCE_LEN);
        }
    }

    #[test]
    fn ciphertext_never_contains_plaintext() {
        let k = test_key(9);
        let ct = k.encrypt("super-secret-api-key").unwrap();
        assert!(!ct.contains("super-secret"));
    }

    #[test]
    fn nonce_is_random_so_same_input_differs() {
        let k = test_key(3);
        assert_ne!(k.encrypt("same").unwrap(), k.encrypt("same").unwrap());
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let ct = test_key(1).encrypt("secret").unwrap();
        assert!(test_key(2).decrypt(&ct).is_err());
    }

    #[test]
    fn plaintext_is_not_mistaken_for_ciphertext() {
        let k = test_key(4);
        // 遗留明文值必须被识别成"不是密文"，调用方才能触发迁移
        for legacy in ["sk-plaintext", "", "v1:not-ours", "myriad-enc:v0:x:y"] {
            assert!(!is_ciphertext(legacy), "{legacy}");
            assert!(k.decrypt(legacy).is_err());
        }
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let k = test_key(5);
        let ct = k.encrypt("secret").unwrap();
        // 翻转密文最后一个 base64 字符 → GCM 认证失败
        let mut bytes: Vec<char> = ct.chars().collect();
        let last = bytes.len() - 1;
        bytes[last] = if bytes[last] == 'A' { 'B' } else { 'A' };
        let tampered: String = bytes.into_iter().collect();
        assert!(k.decrypt(&tampered).is_err());
    }

    #[test]
    fn malformed_ciphertext_does_not_panic() {
        let k = test_key(6);
        for bad in [
            "myriad-enc:v1:",
            "myriad-enc:v1:notbase64",
            "myriad-enc:v1:AAAA:",
            "myriad-enc:v1:AAAA:BBBB",
        ] {
            assert!(k.decrypt(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn fingerprint_is_stable_and_key_specific() {
        assert_eq!(test_key(1).fingerprint(), test_key(1).fingerprint());
        assert_ne!(test_key(1).fingerprint(), test_key(2).fingerprint());
        assert_eq!(test_key(1).fingerprint().len(), 8);
    }

    #[test]
    fn legacy_derivation_matches_historical_federation_scheme() {
        // 这个断言锁住向后兼容：改了它，既有联邦私钥就解不开了
        let mut hasher = Sha256::new();
        hasher.update(b"myriad-federation-key-encryption:");
        hasher.update(b"test-secret");
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(derive_legacy_key("test-secret"), expected);
    }

    #[test]
    fn key_material_parsing_requires_exactly_32_bytes() {
        assert!(parse_key_material(&BASE64.encode([0u8; 32])).is_some());
        assert!(parse_key_material(&BASE64.encode([0u8; 31])).is_none());
        assert!(parse_key_material(&BASE64.encode([0u8; 33])).is_none());
        assert!(parse_key_material("not-base64!!").is_none());
        // 尾部换行是密钥文件的正常形态
        assert!(parse_key_material(&format!("{}\n", BASE64.encode([0u8; 32]))).is_some());
    }

    #[test]
    fn sensitive_key_heuristic_covers_secrets_not_quotas() {
        for secret in [
            "github_token",
            "token",
            "access_token",
            "refresh_token",
            "x_bearer_token",
            "bangumi_access_token",
            "steam_api_key",
            "gemini_api_key",
            "github_client_secret",
            "tencent_secret_key",
            "tencent_secret_id",
            "admin_password",
            "psn_npsso",
            "agora_app_certificate",
            "agora_customer_secret",
        ] {
            assert!(
                is_sensitive_config_key(secret),
                "{secret} must be treated as sensitive"
            );
        }
        for non_secret in [
            "user_ai_daily_tokens",
            "guest_ai_daily_tokens",
            "openai_max_tokens",
            "discord_token_expires_at",
            "github_username",
            "enabled",
            "base_url",
        ] {
            assert!(
                !is_sensitive_config_key(non_secret),
                "{non_secret} must not be encrypted"
            );
        }
    }

    #[test]
    fn write_key_file_refuses_to_clobber() {
        let dir = std::env::temp_dir().join(format!("myriad-key-{}", uuid::Uuid::new_v4()));
        let path = dir.join(".secret-key");
        assert!(write_key_file(&path, "first").is_ok());
        // 第二次必须失败：覆盖密钥 = 把已有密文全部变成不可恢复的垃圾
        let err = write_key_file(&path, "second").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("first"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "key file must be owner-only");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}

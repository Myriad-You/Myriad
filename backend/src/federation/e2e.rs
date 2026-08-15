//! 端到端加密模块（Phase 5 补全 — 安全增强）
//!
//! 基于 X25519 密钥交换 + AES-256-GCM（HKDF 派生）对称加密
//! 用于 Channel 和 Room 消息的可选 E2E 加密
//!
//! 加密流程：
//! 1. 本地生成 X25519 密钥对
//! 2. 通过联邦消息交换公钥（myriad:KeyExchange）
//! 3. ECDH 共享密钥 → HKDF-SHA256 派生对称密钥
//! 4. 使用 AES-256-GCM 加密消息载荷（复用项目 aes-gcm 依赖）
//!
//! 对外算法标识：`x25519-aes256gcm`

use serde::{Deserialize, Serialize};

/// 对外算法标识（Activity / 信封字段）
pub const E2E_ALGORITHM: &str = "x25519-aes256gcm";

// 类型定义

/// E2E 密钥对（X25519）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct E2eKeyPair {
    /// 公钥 (Base64 编码, 32 字节)
    pub public_key: String,
    /// 私钥 (Base64 编码, 32 字节) — 仅本地存储，不发送
    #[serde(skip_serializing)]
    pub private_key: String,
}

/// 加密消息信封
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    /// 加密算法标识
    pub algorithm: String,
    /// 随机 nonce (Base64, 12 字节)
    pub nonce: String,
    /// 发送方公钥 (Base64, 32 字节) — 会话模式下为本方长期公钥
    pub ephemeral_key: String,
    /// 加密后的密文 (Base64)
    pub ciphertext: String,
}

/// Activity `object` for `myriad:KeyExchange` (Channel or Room fan-out).
///
/// Wire shape matches channel/room handlers: camelCase `publicKey` + algorithm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyExchangePayload {
    #[serde(rename = "type")]
    pub payload_type: String,
    /// Set exactly one of channel / room
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    pub public_key: String,
    pub algorithm: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

impl KeyExchangePayload {
    pub fn for_channel(channel_id: &str, public_key: &str, timestamp: Option<String>) -> Self {
        Self {
            payload_type: "myriad:KeyExchange".to_string(),
            channel: Some(channel_id.to_string()),
            room: None,
            public_key: public_key.to_string(),
            algorithm: E2E_ALGORITHM.to_string(),
            timestamp,
        }
    }

    pub fn for_room(room_id: &str, public_key: &str, timestamp: Option<String>) -> Self {
        Self {
            payload_type: "myriad:KeyExchange".to_string(),
            channel: None,
            room: Some(room_id.to_string()),
            public_key: public_key.to_string(),
            algorithm: E2E_ALGORITHM.to_string(),
            timestamp,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// 加密会话状态
#[derive(Debug, Clone)]
pub struct EncryptionSession {
    /// Channel 或 Room ID
    pub target_id: String,
    /// 本方密钥对
    pub local_keypair: E2eKeyPair,
    /// 对方公钥
    pub remote_public_key: Option<String>,
    /// 是否已完成密钥交换
    pub established: bool,
}

// 密钥生成

/// 生成 X25519 密钥对
///
/// 使用 CSPRNG + x25519-dalek 生成标准 Curve25519 密钥材料。
/// x25519-dalek 3: prefer `StaticSecret::random()` via the crate's `getrandom`
/// feature (avoids call-site coupling to a specific `rand_core` trait major).
pub fn generate_keypair() -> E2eKeyPair {
    use x25519_dalek::{PublicKey, StaticSecret};

    let secret = StaticSecret::random();
    let public = PublicKey::from(&secret);

    E2eKeyPair {
        public_key: base64_encode(public.as_bytes()),
        private_key: base64_encode(secret.as_bytes()),
    }
}

/// 执行 X25519 ECDH 密钥交换，得到共享密钥
pub fn compute_shared_secret(local_private: &[u8; 32], remote_public: &[u8; 32]) -> [u8; 32] {
    use x25519_dalek::{PublicKey, StaticSecret};

    let secret = StaticSecret::from(*local_private);
    let public = PublicKey::from(*remote_public);
    *secret.diffie_hellman(&public).as_bytes()
}

// AES-256-GCM 对称加密（HKDF 派生）

/// 使用共享密钥加密消息（AES-256-GCM AEAD）
///
/// `aad` 绑定 channel/room id，防止密文跨通道重放。
pub fn encrypt_message(
    plaintext: &[u8],
    shared_secret: &[u8; 32],
    sender_ephemeral_pk: &[u8; 32],
    aad: &[u8],
) -> Result<EncryptedEnvelope, String> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};

    let encryption_key = hkdf_derive(shared_secret, b"mfp-e2e-aes256gcm");

    // rand 0.10 already in tree; avoid older rand_core OsRng trait paths for nonces.
    let nonce_bytes: [u8; 12] = rand::random();

    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&encryption_key)
        .map_err(|e| format!("Cipher init failed: {}", e))?;
    let nonce = aes_gcm::Nonce::from(nonce_bytes);

    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|e| format!("Encryption failed: {}", e))?;

    Ok(EncryptedEnvelope {
        algorithm: E2E_ALGORITHM.to_string(),
        nonce: base64_encode(&nonce_bytes),
        ephemeral_key: base64_encode(sender_ephemeral_pk),
        ciphertext: base64_encode(&ciphertext),
    })
}

/// 使用共享密钥解密消息
pub fn decrypt_message(
    envelope: &EncryptedEnvelope,
    shared_secret: &[u8; 32],
    aad: &[u8],
) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};

    let encryption_key = hkdf_derive(shared_secret, b"mfp-e2e-aes256gcm");

    let nonce_bytes = base64_decode(&envelope.nonce).map_err(|_| "Invalid nonce encoding")?;
    let ciphertext =
        base64_decode(&envelope.ciphertext).map_err(|_| "Invalid ciphertext encoding")?;

    if nonce_bytes.len() != 12 {
        return Err("Invalid nonce length".to_string());
    }

    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&encryption_key)
        .map_err(|e| format!("Cipher init failed: {}", e))?;
    let nonce_bytes: [u8; 12] = nonce_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Invalid nonce length".to_string())?;
    let nonce = aes_gcm::Nonce::from(nonce_bytes);

    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext.as_ref(),
                aad,
            },
        )
        .map_err(|e| format!("Decryption failed: {}", e))
}

// 辅助函数

/// HKDF-SHA256 简化实现（Extract + Expand 单步）
fn hkdf_derive(ikm: &[u8], info: &[u8]) -> [u8; 32] {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    // Extract: PRK = HMAC-SHA256(salt=0x00..., IKM)
    let salt = [0u8; 32];
    let mut extractor = HmacSha256::new_from_slice(&salt).expect("HMAC can accept any key length");
    extractor.update(ikm);
    let prk = extractor.finalize().into_bytes();

    // Expand: OKM = HMAC-SHA256(PRK, info || 0x01)
    let mut expander = HmacSha256::new_from_slice(&prk).expect("HMAC can accept any key length");
    expander.update(info);
    expander.update(&[0x01]);
    let okm = expander.finalize().into_bytes();

    let mut key = [0u8; 32];
    key.copy_from_slice(&okm);
    key
}

/// Base64 编码
fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Base64 解码
fn base64_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s)
}

// 会话管理 API

/// 创建新的加密会话（生成密钥对）
pub fn create_session(target_id: &str) -> EncryptionSession {
    let kp = generate_keypair();
    EncryptionSession {
        target_id: target_id.to_string(),
        local_keypair: kp,
        remote_public_key: None,
        established: false,
    }
}

/// 处理收到的密钥交换载荷，完成会话建立
pub fn accept_key_exchange(
    session: &mut EncryptionSession,
    remote_pk_base64: &str,
) -> Result<[u8; 32], String> {
    let remote_pk_bytes =
        base64_decode(remote_pk_base64).map_err(|_| "Invalid remote public key encoding")?;
    if remote_pk_bytes.len() != 32 {
        return Err("Remote public key must be 32 bytes".to_string());
    }

    session.remote_public_key = Some(remote_pk_base64.to_string());
    session.established = true;

    let local_sk_bytes = base64_decode(&session.local_keypair.private_key)
        .map_err(|_| "Invalid local private key")?;
    if local_sk_bytes.len() != 32 {
        return Err("Local private key must be 32 bytes".to_string());
    }

    let mut sk = [0u8; 32];
    sk.copy_from_slice(&local_sk_bytes);
    let mut rpk = [0u8; 32];
    rpk.copy_from_slice(&remote_pk_bytes);

    Ok(compute_shared_secret(&sk, &rpk))
}

/// 使用已建立的会话加密消息
pub fn encrypt_with_session(
    session: &EncryptionSession,
    plaintext: &[u8],
) -> Result<EncryptedEnvelope, String> {
    if !session.established {
        return Err("Session not established".to_string());
    }

    let remote_pk = session
        .remote_public_key
        .as_ref()
        .ok_or("No remote public key")?;

    let local_sk_bytes = base64_decode(&session.local_keypair.private_key)
        .map_err(|_| "Invalid local private key")?;
    let remote_pk_bytes = base64_decode(remote_pk).map_err(|_| "Invalid remote public key")?;

    let mut sk = [0u8; 32];
    sk.copy_from_slice(&local_sk_bytes);
    let mut rpk = [0u8; 32];
    rpk.copy_from_slice(&remote_pk_bytes);

    let shared = compute_shared_secret(&sk, &rpk);

    let local_pk_bytes =
        base64_decode(&session.local_keypair.public_key).map_err(|_| "Invalid local public key")?;
    let mut epk = [0u8; 32];
    epk.copy_from_slice(&local_pk_bytes);

    encrypt_message(plaintext, &shared, &epk, session.target_id.as_bytes())
}

/// 解密收到的加密消息
///
/// 信封里的 `ephemeral_key` 是**发送方**的公钥。收到对端消息时它就是我们要的
/// ECDH 对端；但自己发出去的消息，这个字段等于本地公钥 —— 拿它做 ECDH 得到的是
/// ECDH(local_sk, local_pk)，和加密时用的 ECDH(local_sk, remote_pk) 不是同一个
/// 密钥，于是**发送方永远解不开自己发的消息**。Channel 聊天里表现为：自己的气泡
/// 一直停在「Encrypted · decrypting…」，对端却读得正常。
///
/// 因此按顺序试：信封里的发送方公钥（若不是我们自己）→ 会话缓存的
/// remote_public_key。两个都试是为了兼容对端刚轮换、我们还没记下新公钥的情况。
pub fn decrypt_with_session(
    session: &EncryptionSession,
    envelope: &EncryptedEnvelope,
) -> Result<Vec<u8>, String> {
    let local_pk = session.local_keypair.public_key.as_str();
    let mut candidates: Vec<&str> = Vec::with_capacity(2);
    if !envelope.ephemeral_key.is_empty() && envelope.ephemeral_key != local_pk {
        candidates.push(envelope.ephemeral_key.as_str());
    }
    if let Some(remote) = session.remote_public_key.as_deref() {
        if !candidates.contains(&remote) {
            candidates.push(remote);
        }
    }
    if candidates.is_empty() {
        return Err("No remote public key for decryption".to_string());
    }

    let local_sk_bytes = base64_decode(&session.local_keypair.private_key)
        .map_err(|_| "Invalid local private key")?;
    if local_sk_bytes.len() != 32 {
        return Err("Local private key must be 32 bytes".to_string());
    }
    let mut sk = [0u8; 32];
    sk.copy_from_slice(&local_sk_bytes);

    let mut last_err = "No remote public key for decryption".to_string();
    for candidate in candidates {
        let remote_pk_bytes = match base64_decode(candidate) {
            Ok(b) if b.len() == 32 => b,
            Ok(_) => {
                last_err = "Remote public key must be 32 bytes".to_string();
                continue;
            }
            Err(_) => {
                last_err = "Invalid remote/ephemeral key encoding".to_string();
                continue;
            }
        };
        let mut rpk = [0u8; 32];
        rpk.copy_from_slice(&remote_pk_bytes);
        let shared = compute_shared_secret(&sk, &rpk);
        match decrypt_message(envelope, &shared, session.target_id.as_bytes()) {
            Ok(plain) => return Ok(plain),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// 校验 Base64 编码的 32 字节 X25519 公钥
pub fn validate_public_key_b64(public_key: &str) -> Result<(), String> {
    let bytes = base64_decode(public_key).map_err(|_| "Invalid public key encoding")?;
    if bytes.len() != 32 {
        return Err("Public key must be 32 bytes".to_string());
    }
    Ok(())
}

/// 从已持久化的密钥材料重建会话（例如 channel.properties.e2e）
pub fn session_from_stored(
    target_id: &str,
    local_public_key: &str,
    local_private_key: &str,
    remote_public_key: Option<&str>,
) -> Result<EncryptionSession, String> {
    // Validate encodings early so callers get clear errors
    let lpk = base64_decode(local_public_key).map_err(|_| "Invalid local public key encoding")?;
    let lsk = base64_decode(local_private_key).map_err(|_| "Invalid local private key encoding")?;
    if lpk.len() != 32 || lsk.len() != 32 {
        return Err("Local E2E keys must be 32-byte X25519 material".to_string());
    }
    if let Some(rpk) = remote_public_key {
        let bytes = base64_decode(rpk).map_err(|_| "Invalid remote public key encoding")?;
        if bytes.len() != 32 {
            return Err("Remote public key must be 32 bytes".to_string());
        }
    }

    Ok(EncryptionSession {
        target_id: target_id.to_string(),
        local_keypair: E2eKeyPair {
            public_key: local_public_key.to_string(),
            private_key: local_private_key.to_string(),
        },
        remote_public_key: remote_public_key.map(|s| s.to_string()),
        established: remote_public_key.is_some(),
    })
}

/// 将明文 JSON 载荷加密为可序列化信封 Value
pub fn encrypt_json_payload(
    session: &EncryptionSession,
    payload: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let plaintext = serde_json::to_vec(payload).map_err(|e| format!("payload serialize: {e}"))?;
    let envelope = encrypt_with_session(session, &plaintext)?;
    serde_json::to_value(envelope).map_err(|e| format!("envelope serialize: {e}"))
}

/// 将加密信封 Value 解密回 JSON 载荷
pub fn decrypt_json_payload(
    session: &EncryptionSession,
    encrypted_payload: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let envelope: EncryptedEnvelope = serde_json::from_value(encrypted_payload.clone())
        .map_err(|e| format!("envelope parse: {e}"))?;
    let plain = decrypt_with_session(session, &envelope)?;
    serde_json::from_slice(&plain).map_err(|e| format!("plaintext json parse: {e}"))
}

// Room 多方加密（content-key + 按成员 key-wrap）

/// 发给某一收件人的 content-key 包装
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyWrap {
    /// 收件人 X25519 公钥（Base64）
    pub recipient_public_key: String,
    /// 发送方临时公钥
    pub ephemeral_key: String,
    pub nonce: String,
    /// 被包装的 32 字节 content key（密文 Base64）
    pub wrapped_key: String,
}

/// 多方加密信封：一份密文 + 多个 key-wrap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiRecipientEnvelope {
    pub algorithm: String,
    /// 内容加密 nonce
    pub nonce: String,
    pub ciphertext: String,
    pub key_wraps: Vec<KeyWrap>,
}

/// 用随机 content-key 加密明文，并为每个收件人公钥包装 content-key
///
/// `recipients`：`(actor_hint, public_key_b64)` — actor_hint 仅用于错误信息
pub fn encrypt_for_recipients(
    plaintext: &[u8],
    aad: &[u8],
    recipients: &[(String, String)],
) -> Result<MultiRecipientEnvelope, String> {
    if recipients.is_empty() {
        return Err("At least one recipient public key is required".to_string());
    }

    // 1) 随机 content key
    use rand::Rng;
    let mut rng = rand::rng();
    let mut content_key = [0u8; 32];
    rng.fill_bytes(&mut content_key);

    // 2) 用 content key 加密正文（HKDF 派生 AEAD key）
    let content_aad = [aad, b"|content"].concat();
    let body = encrypt_message(plaintext, &content_key, &[0u8; 32], &content_aad)?;
    // body.ephemeral_key is zero placeholder — multi-recipient uses key_wraps instead

    // 3) 为每个收件人 ECDH-wrap content_key
    let mut key_wraps = Vec::with_capacity(recipients.len());
    for (hint, rpk_b64) in recipients {
        validate_public_key_b64(rpk_b64).map_err(|e| format!("recipient {hint}: {e}"))?;

        // 每次 wrap 用独立临时密钥
        let eph = generate_keypair();
        let mut eph_sk = [0u8; 32];
        let mut eph_pk = [0u8; 32];
        let mut rpk = [0u8; 32];
        eph_sk.copy_from_slice(&base64_decode(&eph.private_key).map_err(|_| "eph sk")?);
        eph_pk.copy_from_slice(&base64_decode(&eph.public_key).map_err(|_| "eph pk")?);
        rpk.copy_from_slice(
            &base64_decode(rpk_b64).map_err(|_| format!("recipient {hint} pk decode"))?,
        );

        let shared = compute_shared_secret(&eph_sk, &rpk);
        let wrap_aad = [aad, b"|wrap:", rpk_b64.as_bytes()].concat();
        let wrapped = encrypt_message(&content_key, &shared, &eph_pk, &wrap_aad)
            .map_err(|e| format!("key wrap for {hint}: {e}"))?;

        key_wraps.push(KeyWrap {
            recipient_public_key: rpk_b64.clone(),
            ephemeral_key: wrapped.ephemeral_key,
            nonce: wrapped.nonce,
            wrapped_key: wrapped.ciphertext,
        });
    }

    Ok(MultiRecipientEnvelope {
        algorithm: E2E_ALGORITHM.to_string(),
        nonce: body.nonce,
        ciphertext: body.ciphertext,
        key_wraps,
    })
}

/// 使用本地私钥从多方信封中解出明文
pub fn decrypt_for_recipient(
    envelope: &MultiRecipientEnvelope,
    local_private_key_b64: &str,
    local_public_key_b64: &str,
    aad: &[u8],
) -> Result<Vec<u8>, String> {
    let wrap = envelope
        .key_wraps
        .iter()
        .find(|w| w.recipient_public_key == local_public_key_b64)
        .ok_or("No key wrap for local public key")?;

    let mut local_sk = [0u8; 32];
    local_sk.copy_from_slice(
        &base64_decode(local_private_key_b64).map_err(|_| "Invalid local private key")?,
    );
    if local_sk.len() != 32 {
        return Err("Local private key must be 32 bytes".to_string());
    }

    let eph_pk_bytes =
        base64_decode(&wrap.ephemeral_key).map_err(|_| "Invalid wrap ephemeral key")?;
    if eph_pk_bytes.len() != 32 {
        return Err("Wrap ephemeral key must be 32 bytes".to_string());
    }
    let mut eph_pk = [0u8; 32];
    eph_pk.copy_from_slice(&eph_pk_bytes);

    let shared = compute_shared_secret(&local_sk, &eph_pk);
    let wrap_aad = [aad, b"|wrap:", local_public_key_b64.as_bytes()].concat();
    let wrap_env = EncryptedEnvelope {
        algorithm: envelope.algorithm.clone(),
        nonce: wrap.nonce.clone(),
        ephemeral_key: wrap.ephemeral_key.clone(),
        ciphertext: wrap.wrapped_key.clone(),
    };
    let content_key_bytes = decrypt_message(&wrap_env, &shared, &wrap_aad)?;
    if content_key_bytes.len() != 32 {
        return Err("Unwrapped content key must be 32 bytes".to_string());
    }
    let mut content_key = [0u8; 32];
    content_key.copy_from_slice(&content_key_bytes);

    let content_aad = [aad, b"|content"].concat();
    let body_env = EncryptedEnvelope {
        algorithm: envelope.algorithm.clone(),
        nonce: envelope.nonce.clone(),
        ephemeral_key: String::new(),
        ciphertext: envelope.ciphertext.clone(),
    };
    decrypt_message(&body_env, &content_key, &content_aad)
}

/// JSON 载荷多方加密
pub fn encrypt_json_for_recipients(
    payload: &serde_json::Value,
    aad: &[u8],
    recipients: &[(String, String)],
) -> Result<serde_json::Value, String> {
    let plaintext = serde_json::to_vec(payload).map_err(|e| format!("payload serialize: {e}"))?;
    let envelope = encrypt_for_recipients(&plaintext, aad, recipients)?;
    serde_json::to_value(envelope).map_err(|e| format!("envelope serialize: {e}"))
}

/// JSON 载荷多方解密
pub fn decrypt_json_for_recipient(
    encrypted_payload: &serde_json::Value,
    local_private_key_b64: &str,
    local_public_key_b64: &str,
    aad: &[u8],
) -> Result<serde_json::Value, String> {
    let envelope: MultiRecipientEnvelope = serde_json::from_value(encrypted_payload.clone())
        .map_err(|e| format!("multi envelope parse: {e}"))?;
    let plain = decrypt_for_recipient(&envelope, local_private_key_b64, local_public_key_b64, aad)?;
    serde_json::from_slice(&plain).map_err(|e| format!("plaintext json parse: {e}"))
}

// At-rest private key sealing
// AES-256-GCM with key = SHA-256("myriad-e2e-key-seal:" || jwt_secret)
// Stored form: "sealed:v1:" + base64(nonce || ciphertext)
// Legacy plaintext base64 private keys still load for one-release migration.

const E2E_SK_SEAL_PREFIX: &str = "sealed:v1:";
/// KDF domain for current seals (channel + room)
const E2E_SEAL_KDF_LABEL: &[u8] = b"myriad-e2e-key-seal:";
/// Brief room-only label used before helpers were shared — still accepted on unseal.
const E2E_SEAL_KDF_LABEL_LEGACY_ROOM: &[u8] = b"myriad-room-e2e-key-seal:";

fn derive_seal_aes_key(jwt_secret: &str, label: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(label);
    hasher.update(jwt_secret.as_bytes());
    hasher.finalize().into()
}

/// Seal an E2E X25519 private key (base64) for DB storage.
pub fn seal_private_key(plain_b64: &str, jwt_secret: &str) -> Result<String, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    let key = derive_seal_aes_key(jwt_secret, E2E_SEAL_KDF_LABEL);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let nonce_bytes = rand::random::<[u8; 12]>();
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, plain_b64.as_bytes())
        .map_err(|e| format!("e2e seal failed: {e}"))?;
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Ok(format!("{}{}", E2E_SK_SEAL_PREFIX, B64.encode(&combined)))
}

/// Unseal a stored E2E private key. Plain (legacy) values pass through.
pub fn unseal_private_key(stored: &str, jwt_secret: &str) -> Result<String, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    let Some(rest) = stored.strip_prefix(E2E_SK_SEAL_PREFIX) else {
        return Ok(stored.to_string());
    };
    let combined = B64
        .decode(rest)
        .map_err(|e| format!("e2e unseal b64: {e}"))?;
    if combined.len() < 13 {
        return Err("e2e sealed key too short".into());
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce_bytes: [u8; 12] = nonce_bytes
        .try_into()
        .map_err(|_| "invalid e2e seal nonce".to_string())?;
    let nonce = Nonce::from(nonce_bytes);

    // Try current KDF label, then legacy room-only label.
    for label in [E2E_SEAL_KDF_LABEL, E2E_SEAL_KDF_LABEL_LEGACY_ROOM] {
        let key = derive_seal_aes_key(jwt_secret, label);
        let cipher = match Aes256Gcm::new_from_slice(&key) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Ok(plain) = cipher.decrypt(&nonce, ciphertext) {
            if let Ok(s) = String::from_utf8(plain) {
                return Ok(s);
            }
        }
    }
    Err("e2e unseal failed".into())
}

/// True when value looks like a sealed blob (not legacy plain base64).
#[allow(dead_code)]
pub fn is_sealed_private_key(stored: &str) -> bool {
    stored.starts_with(E2E_SK_SEAL_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keygen_produces_32_byte_material() {
        let kp = generate_keypair();
        let pk = base64_decode(&kp.public_key).unwrap();
        let sk = base64_decode(&kp.private_key).unwrap();
        assert_eq!(pk.len(), 32);
        assert_eq!(sk.len(), 32);
        assert_ne!(pk, sk);
    }

    /// HKDF (HMAC-SHA256) derives a 32-byte key and is deterministic for fixed IKM/info.
    #[test]
    fn hkdf_hmac_sha256_is_deterministic() {
        let ikm = [7u8; 32];
        let a = hkdf_derive(&ikm, b"mfp-e2e-aes256gcm");
        let b = hkdf_derive(&ikm, b"mfp-e2e-aes256gcm");
        let c = hkdf_derive(&ikm, b"other-info");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, [0u8; 32]);
    }

    #[test]
    fn seal_unseal_roundtrip_and_legacy_plain() {
        let secret = "test-jwt-secret";
        let plain = "dGVzdC1wcml2YXRlLWtleS1iYXNlNjQ=";
        let sealed = seal_private_key(plain, secret).unwrap();
        assert!(is_sealed_private_key(&sealed));
        assert_eq!(unseal_private_key(&sealed, secret).unwrap(), plain);
        // Legacy plain passes through
        assert_eq!(unseal_private_key(plain, secret).unwrap(), plain);
        // Wrong secret fails
        assert!(unseal_private_key(&sealed, "other").is_err());
    }

    #[test]
    fn ecdh_is_symmetric_between_peers() {
        // x25519-dalek 3: StaticSecret::random + From<[u8;32]> + diffie_hellman
        // must remain symmetric for channel/room E2E key exchange.
        let alice = generate_keypair();
        let bob = generate_keypair();

        let mut a_sk = [0u8; 32];
        let mut a_pk = [0u8; 32];
        let mut b_sk = [0u8; 32];
        let mut b_pk = [0u8; 32];
        a_sk.copy_from_slice(&base64_decode(&alice.private_key).unwrap());
        a_pk.copy_from_slice(&base64_decode(&alice.public_key).unwrap());
        b_sk.copy_from_slice(&base64_decode(&bob.private_key).unwrap());
        b_pk.copy_from_slice(&base64_decode(&bob.public_key).unwrap());

        let ab = compute_shared_secret(&a_sk, &b_pk);
        let ba = compute_shared_secret(&b_sk, &a_pk);
        assert_eq!(ab, ba, "ECDH shared secrets must match both directions");
        assert_ne!(ab, [0u8; 32]);
    }

    #[test]
    fn aes_gcm_aad_roundtrip_and_binding() {
        // aes-gcm 0.11: Nonce::from([u8;12]), new_from_slice, Payload { msg, aad }
        let alice = generate_keypair();
        let bob = generate_keypair();
        let mut a_sk = [0u8; 32];
        let mut a_pk = [0u8; 32];
        let mut b_pk = [0u8; 32];
        a_sk.copy_from_slice(&base64_decode(&alice.private_key).unwrap());
        a_pk.copy_from_slice(&base64_decode(&alice.public_key).unwrap());
        b_pk.copy_from_slice(&base64_decode(&bob.public_key).unwrap());

        let shared = compute_shared_secret(&a_sk, &b_pk);
        let aad = b"channel:ch-aad-test";
        let plaintext = b"federation body";
        let envelope = encrypt_message(plaintext, &shared, &a_pk, aad).expect("encrypt");
        assert_eq!(envelope.algorithm, E2E_ALGORITHM);
        assert_eq!(
            decrypt_message(&envelope, &shared, aad).expect("decrypt"),
            plaintext
        );
        // Wrong AAD must fail authentication (channel/room binding).
        assert!(decrypt_message(&envelope, &shared, b"channel:other").is_err());
    }

    #[test]
    fn session_encrypt_decrypt_roundtrip() {
        let mut alice = create_session("ch-1");
        let mut bob = create_session("ch-1");

        let alice_shared = accept_key_exchange(&mut alice, &bob.local_keypair.public_key).unwrap();
        let bob_shared = accept_key_exchange(&mut bob, &alice.local_keypair.public_key).unwrap();
        assert_eq!(alice_shared, bob_shared);

        let plaintext = b"hello federation e2e";
        let envelope = encrypt_with_session(&alice, plaintext).unwrap();
        assert_eq!(envelope.algorithm, E2E_ALGORITHM);

        let recovered = decrypt_with_session(&bob, &envelope).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn json_payload_roundtrip_via_session_from_stored() {
        let mut alice = create_session("ch-json");
        let mut bob = create_session("ch-json");
        accept_key_exchange(&mut alice, &bob.local_keypair.public_key).unwrap();
        accept_key_exchange(&mut bob, &alice.local_keypair.public_key).unwrap();

        let payload = serde_json::json!({"text": "密文消息", "n": 42});
        let encrypted = encrypt_json_payload(&alice, &payload).unwrap();

        // Rebuild bob session from stored material (simulates channel.properties.e2e)
        let bob_restored = session_from_stored(
            "ch-json",
            &bob.local_keypair.public_key,
            &bob.local_keypair.private_key,
            Some(&alice.local_keypair.public_key),
        )
        .unwrap();
        assert!(bob_restored.established);

        let plain = decrypt_json_payload(&bob_restored, &encrypted).unwrap();
        assert_eq!(plain, payload);
    }

    #[test]
    fn wrong_remote_key_fails_decrypt() {
        let mut alice = create_session("ch-bad");
        let mut bob = create_session("ch-bad");
        let eve = create_session("ch-bad");
        accept_key_exchange(&mut alice, &bob.local_keypair.public_key).unwrap();
        accept_key_exchange(&mut bob, &alice.local_keypair.public_key).unwrap();

        let envelope = encrypt_with_session(&alice, b"secret").unwrap();
        // Eve has her own keys — must not decrypt
        let mut eve_sess = eve;
        accept_key_exchange(&mut eve_sess, &alice.local_keypair.public_key).unwrap();
        assert!(decrypt_with_session(&eve_sess, &envelope).is_err());
    }

    #[test]
    fn key_exchange_payload_channel_wire_shape() {
        let session = create_session("ch-kx");
        let payload = KeyExchangePayload::for_channel(
            &session.target_id,
            &session.local_keypair.public_key,
            Some("2026-01-01T00:00:00Z".into()),
        );
        assert_eq!(payload.payload_type, "myriad:KeyExchange");
        assert_eq!(payload.channel.as_deref(), Some("ch-kx"));
        assert!(payload.room.is_none());
        assert!(!payload.public_key.is_empty());
        assert_eq!(payload.algorithm, E2E_ALGORITHM);
        let v = payload.to_json();
        assert_eq!(v["publicKey"], payload.public_key);
        assert_eq!(v["channel"], "ch-kx");
        assert_eq!(v["timestamp"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn key_exchange_payload_room_wire_shape() {
        let session = create_session("rm-kx");
        let payload = KeyExchangePayload::for_room(
            &session.target_id,
            &session.local_keypair.public_key,
            None,
        );
        assert_eq!(payload.room.as_deref(), Some("rm-kx"));
        assert!(payload.channel.is_none());
        assert_eq!(payload.to_json()["room"], "rm-kx");
        assert!(payload.to_json().get("timestamp").is_none());
    }

    #[test]
    fn multi_recipient_encrypt_decrypt_three_peers() {
        let alice = generate_keypair();
        let bob = generate_keypair();
        let carol = generate_keypair();
        let aad = b"room-42";

        let recipients = vec![
            ("alice".into(), alice.public_key.clone()),
            ("bob".into(), bob.public_key.clone()),
            ("carol".into(), carol.public_key.clone()),
        ];
        let plain = serde_json::json!({"text": "room secret", "n": 7});
        let env = encrypt_json_for_recipients(&plain, aad, &recipients).unwrap();

        for (name, kp) in [("alice", &alice), ("bob", &bob), ("carol", &carol)] {
            let got = decrypt_json_for_recipient(&env, &kp.private_key, &kp.public_key, aad)
                .unwrap_or_else(|e| panic!("{name} decrypt failed: {e}"));
            assert_eq!(got, plain, "{name} should recover plaintext");
        }

        // outsider cannot decrypt
        let eve = generate_keypair();
        assert!(decrypt_json_for_recipient(&env, &eve.private_key, &eve.public_key, aad).is_err());
    }

    /// A sender left out of `recipients` cannot read back their own message.
    ///
    /// `collect_room_e2e_recipients` deliberately excludes the sender, so the
    /// send path has to append a self key-wrap. When the local member key is
    /// unreadable that wrap is missing, and the row is stored as ciphertext the
    /// author's own `get_room_messages` can never open — the bubble sits at
    /// "Encrypted · decrypting…" forever while the peer reads it fine. Hence the
    /// plaintext fallback in `room::messages::send_room_message`.
    #[test]
    fn sender_without_self_wrap_cannot_read_own_message() {
        let aad = b"rm_selfwrap";
        let me = generate_keypair();
        let peer = generate_keypair();
        let plain = serde_json::json!({"text": "hi"});

        let peer_only = vec![("peer".to_string(), peer.public_key.clone())];
        let env = encrypt_json_for_recipients(&plain, aad, &peer_only).unwrap();
        assert!(
            decrypt_json_for_recipient(&env, &me.private_key, &me.public_key, aad).is_err(),
            "sender must not be able to open an envelope with no wrap for its own key"
        );
        assert_eq!(
            decrypt_json_for_recipient(&env, &peer.private_key, &peer.public_key, aad).unwrap(),
            plain,
            "peer still reads it — this is why the failure is invisible to the sender"
        );

        // With the self-wrap appended, both sides read it.
        let with_self = vec![
            ("peer".to_string(), peer.public_key.clone()),
            ("me".to_string(), me.public_key.clone()),
        ];
        let env = encrypt_json_for_recipients(&plain, aad, &with_self).unwrap();
        assert_eq!(
            decrypt_json_for_recipient(&env, &me.private_key, &me.public_key, aad).unwrap(),
            plain
        );
    }

    /// The author of a channel message must be able to read it back.
    ///
    /// `encrypt_with_session` stamps the envelope's `ephemeral_key` with the
    /// *sender's* public key. `decrypt_with_session` used to always ECDH against
    /// that field, so decrypting your own message computed
    /// ECDH(local_sk, local_pk) instead of ECDH(local_sk, remote_pk) and failed.
    /// Every message you sent stayed sealed in your own transcript — "Encrypted ·
    /// decrypting…" forever — while the peer read it normally.
    #[test]
    fn sender_can_decrypt_own_channel_message() {
        let target = "ch_selfread";
        let peer = generate_keypair();

        let mut mine = create_session(target);
        accept_key_exchange(&mut mine, &peer.public_key).unwrap();

        let plain = serde_json::json!({"text": "hello from me"});
        let env = encrypt_json_payload(&mine, &plain).unwrap();

        // The envelope advertises our own key as the sender key.
        let envelope: EncryptedEnvelope = serde_json::from_value(env.clone()).unwrap();
        assert_eq!(envelope.ephemeral_key, mine.local_keypair.public_key);

        assert_eq!(
            decrypt_json_payload(&mine, &env).unwrap(),
            plain,
            "author must be able to reopen their own message"
        );

        // And the peer still reads it with the symmetric ECDH secret.
        let peer_session = session_from_stored(
            target,
            &peer.public_key,
            &peer.private_key,
            Some(&mine.local_keypair.public_key),
        )
        .unwrap();
        assert_eq!(decrypt_json_payload(&peer_session, &env).unwrap(), plain);
    }

    /// A rotated local keypair retires every message encrypted under the old one.
    ///
    /// This is what the lost-update on `properties.e2e` / `shared_data_config.e2e`
    /// caused: the inbound and outbound key-exchange writers each wrote back a
    /// whole-object snapshot, so one silently dropped the other's key material.
    /// Losing `local_private_key` made the next initiate mint a fresh pair and
    /// re-announce it, retiring the history on every turn. Both writers now take
    /// the row lock, so the pair survives a concurrent exchange.
    #[test]
    fn rotated_keypair_cannot_open_prior_ciphertext() {
        let target = "ch_rotate";
        let peer = generate_keypair();

        let mut first = create_session(target);
        accept_key_exchange(&mut first, &peer.public_key).unwrap();
        let plain = serde_json::json!({"text": "before rotation"});
        let env = encrypt_json_payload(&first, &plain).unwrap();
        assert_eq!(decrypt_json_payload(&first, &env).unwrap(), plain);

        // Same peer, new local keypair — neither ECDH candidate reproduces the
        // secret the old private key produced.
        let mut rotated = create_session(target);
        accept_key_exchange(&mut rotated, &peer.public_key).unwrap();
        assert!(
            decrypt_json_payload(&rotated, &env).is_err(),
            "history encrypted before the rotation must not be readable after it"
        );
    }
}

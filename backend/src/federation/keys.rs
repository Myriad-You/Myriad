//! 联邦密钥管理
//!
//! RSA-2048 密钥对的生成、加密存储、加载与轮换。
//! 私钥使用 AES-256-GCM 加密后存储到数据库。
//!
//! 加密密钥来自 [`crate::services::data_key`]，**不再**从 `JWT_SECRET` 派生。
//! 旧格式（v0，JWT_SECRET 派生）仍可解密，并在启动时由
//! [`rewrap_legacy_private_keys`] 重新封装成 v1 —— 之后轮换 `JWT_SECRET`
//! 就不会再让实例丢掉 ActivityPub 身份。

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rsa::pkcs8::{
    DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding,
};
use rsa::{Pkcs1v15Sign, RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};

/// RSA 密钥位长度
const RSA_KEY_BITS: usize = 2048;

/// AES-256-GCM nonce 长度
const AES_NONCE_LEN: usize = 12;

/// 密钥对（内存中明文持有）
#[derive(Clone)]
pub struct KeyPair {
    pub private_key: RsaPrivateKey,
    pub public_key: RsaPublicKey,
}

impl KeyPair {
    /// 生成新的 RSA-2048 密钥对
    pub fn generate() -> Result<Self> {
        let mut rng = rand::rng();
        let private_key =
            RsaPrivateKey::new(&mut rng, RSA_KEY_BITS).context("Failed to generate RSA key")?;
        let public_key = RsaPublicKey::from(&private_key);
        Ok(Self {
            private_key,
            public_key,
        })
    }

    /// 导出公钥为 PEM 格式
    pub fn public_key_pem(&self) -> Result<String> {
        self.public_key
            .to_public_key_pem(LineEnding::LF)
            .context("Failed to encode public key PEM")
    }

    /// 导出私钥为 PEM 格式（明文，仅用于加密前）
    fn private_key_pem(&self) -> Result<String> {
        self.private_key
            .to_pkcs8_pem(LineEnding::LF)
            .map(|s| s.to_string())
            .context("Failed to encode private key PEM")
    }

    /// 加密私钥用于数据库存储（v1 信封）
    ///
    /// 使用 [`crate::services::data_key`] 的数据密钥，**不再**从 `JWT_SECRET` 派生。
    /// 这样轮换 `JWT_SECRET`（session 密钥泄露后的标准处置）不会再让实例丢掉
    /// ActivityPub 身份和全部既有关注关系。
    pub fn encrypt_private_key(&self) -> Result<String> {
        let pem = self.private_key_pem()?;
        crate::services::data_key::data_key().encrypt(&pem)
    }

    /// 从加密存储恢复密钥对。
    ///
    /// 依次尝试两种格式：
    ///
    /// - **v1** `myriad-enc:v1:…` —— 当前格式，数据密钥。
    /// - **v0** 裸 `base64(nonce || ct)` —— 历史格式，密钥由 `JWT_SECRET` 派生。
    ///
    /// 保留 v0 解密路径是升级不炸的前提：已有部署库里全是 v0 密文，
    /// 必须在重新封装完成前一直可读。
    pub fn from_encrypted(
        public_key_pem: &str,
        encrypted_private_key: &str,
        legacy_jwt_secret: &str,
    ) -> Result<Self> {
        let public_key = RsaPublicKey::from_public_key_pem(public_key_pem)
            .context("Failed to parse public key PEM")?;

        let pem = decrypt_private_key_pem(encrypted_private_key, legacy_jwt_secret)?;
        let private_key =
            RsaPrivateKey::from_pkcs8_pem(&pem).context("Failed to parse private key PEM")?;

        Ok(Self {
            private_key,
            public_key,
        })
    }

    /// 使用私钥对数据签名（RSA-SHA256）
    pub fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        let digest = Sha256::digest(data);
        let signature = self
            .private_key
            .sign(Pkcs1v15Sign::new::<Sha256>(), &digest)
            .context("RSA signing failed")?;
        Ok(signature.to_vec())
    }

    /// 使用公钥验证签名
    pub fn verify(public_key_pem: &str, data: &[u8], signature: &[u8]) -> Result<bool> {
        let public_key = RsaPublicKey::from_public_key_pem(public_key_pem)
            .context("Failed to parse public key for verification")?;
        let digest = Sha256::digest(data);
        match public_key.verify(Pkcs1v15Sign::new::<Sha256>(), &digest, signature) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

/// 存储的私钥密文是否还是 v0（JWT_SECRET 派生）格式。
///
/// 用来挑出需要重新封装的行。
pub fn is_legacy_ciphertext(stored: &str) -> bool {
    !crate::services::data_key::is_ciphertext(stored)
}

/// 解密私钥 PEM：先试 v1，失败再试 v0。
fn decrypt_private_key_pem(stored: &str, legacy_jwt_secret: &str) -> Result<String> {
    if crate::services::data_key::is_ciphertext(stored) {
        return crate::services::data_key::data_key().decrypt(stored);
    }
    decrypt_legacy_private_key(stored, legacy_jwt_secret)
}

/// v0 解密：`base64(nonce || ciphertext)`，密钥 = SHA-256("myriad-federation-key-encryption:" || jwt_secret)
fn decrypt_legacy_private_key(stored: &str, jwt_secret: &str) -> Result<String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

    let combined = BASE64
        .decode(stored)
        .context("Failed to decode base64 encrypted key")?;

    if combined.len() < AES_NONCE_LEN + 1 {
        return Err(anyhow::anyhow!("Encrypted key data too short"));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(AES_NONCE_LEN);
    let nonce_bytes: [u8; AES_NONCE_LEN] =
        nonce_bytes.try_into().context("Invalid AES nonce length")?;
    let aes_key = crate::services::data_key::derive_legacy_key(jwt_secret);
    let cipher = Aes256Gcm::new_from_slice(&aes_key).context("Failed to create AES cipher")?;

    let plaintext = cipher
        .decrypt(&Nonce::from(nonce_bytes), ciphertext)
        .map_err(|e| anyhow::anyhow!("AES-GCM decryption failed: {}", e))?;

    String::from_utf8(plaintext).context("Decrypted key is not valid UTF-8")
}

/// 把 `federation_keys` 里的 v0 私钥密文重新封装成 v1。
///
/// 启动时跑一次。**必须**在这里一次性做完，不能像配置那样惰性迁移：
/// 私钥要随时可用于签名，不能出现"下次写入时才升级"的空窗。
///
/// 逐行推进且幂等 —— 单行失败只跳过该行，下次启动重试；已是 v1 的行直接跳过。
pub async fn rewrap_legacy_private_keys(
    db: &sea_orm::DatabaseConnection,
    legacy_jwt_secret: &str,
) -> Result<usize> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    if crate::services::data_key::data_key().source().is_fallback() {
        // 兜底密钥本身就是 JWT_SECRET 派生的，重新封装没有任何收益。
        return Ok(0);
    }

    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT user_id, private_key_encrypted FROM federation_keys".to_string(),
        ))
        .await
        .context("Failed to read federation keys for rewrap")?;

    let mut rewrapped = 0usize;
    for row in rows {
        let (Ok(user_id), Ok(stored)) = (
            row.try_get::<i32>("", "user_id"),
            row.try_get::<String>("", "private_key_encrypted"),
        ) else {
            continue;
        };
        if !is_legacy_ciphertext(&stored) {
            continue;
        }

        let pem = match decrypt_legacy_private_key(&stored, legacy_jwt_secret) {
            Ok(pem) => pem,
            Err(e) => {
                // 通常意味着 JWT_SECRET 已经被换过 —— 这把私钥本来就已经不可恢复。
                tracing::error!(
                    user_id,
                    "Cannot decrypt legacy federation key for rewrap: {e}"
                );
                continue;
            }
        };

        let sealed = match crate::services::data_key::data_key().encrypt(&pem) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(user_id, "Failed to re-encrypt federation key: {e}");
                continue;
            }
        };

        match db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE federation_keys SET private_key_encrypted = $2 WHERE user_id = $1",
                vec![user_id.into(), sealed.into()],
            ))
            .await
        {
            Ok(_) => rewrapped += 1,
            Err(e) => tracing::error!(user_id, "Failed to store rewrapped federation key: {e}"),
        }
    }

    if rewrapped > 0 {
        tracing::info!(
            "🔐 Re-wrapped {rewrapped} federation private key(s); JWT_SECRET can now be rotated safely"
        );
    }
    Ok(rewrapped)
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("public_key", &"[RSA Public Key]")
            .field("private_key", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation_and_pem_export() {
        let kp = KeyPair::generate().unwrap();
        let pem = kp.public_key_pem().unwrap();
        assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----"));
        assert!(pem.ends_with("-----END PUBLIC KEY-----\n"));
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let kp = KeyPair::generate().unwrap();
        let secret = "test-jwt-secret-that-is-long-enough-for-production";

        let encrypted = kp.encrypt_private_key().unwrap();
        let public_pem = kp.public_key_pem().unwrap();

        let restored = KeyPair::from_encrypted(&public_pem, &encrypted, secret).unwrap();
        assert_eq!(
            kp.public_key_pem().unwrap(),
            restored.public_key_pem().unwrap()
        );
    }

    /// 用历史算法产出 v0 密文，模拟已有部署库里的数据。
    fn encrypt_v0(kp: &KeyPair, jwt_secret: &str) -> String {
        use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

        let pem = kp.private_key_pem().unwrap();
        let aes_key = crate::services::data_key::derive_legacy_key(jwt_secret);
        let cipher = Aes256Gcm::new_from_slice(&aes_key).unwrap();
        let nonce_bytes = rand::random::<[u8; AES_NONCE_LEN]>();
        let ciphertext = cipher
            .encrypt(&Nonce::from(nonce_bytes), pem.as_bytes())
            .unwrap();
        let mut combined = Vec::with_capacity(AES_NONCE_LEN + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);
        BASE64.encode(&combined)
    }

    /// 升级不炸的核心保证：库里的 v0 密文必须继续可读。
    ///
    /// 这条断言一旦失守，已有部署升级后就会丢掉 ActivityPub 身份和全部
    /// 既有关注关系（远端认的是那把公钥）。
    #[test]
    fn legacy_v0_ciphertext_still_decrypts_after_upgrade() {
        let kp = KeyPair::generate().unwrap();
        let secret = "existing-deployment-jwt-secret";
        let v0 = encrypt_v0(&kp, secret);

        assert!(is_legacy_ciphertext(&v0), "v0 must be detected as legacy");

        let restored = KeyPair::from_encrypted(&kp.public_key_pem().unwrap(), &v0, secret).unwrap();
        assert_eq!(
            kp.public_key_pem().unwrap(),
            restored.public_key_pem().unwrap()
        );
    }

    #[test]
    fn v1_ciphertext_is_not_flagged_as_legacy() {
        let kp = KeyPair::generate().unwrap();
        let v1 = kp.encrypt_private_key().unwrap();
        assert!(!is_legacy_ciphertext(&v1));
        // v1 不依赖 JWT_SECRET —— 传一个完全不同的 secret 也必须解得开，
        // 这正是"轮换 JWT_SECRET 不再丢身份"的含义。
        let restored = KeyPair::from_encrypted(
            &kp.public_key_pem().unwrap(),
            &v1,
            "a-totally-rotated-secret",
        )
        .unwrap();
        assert_eq!(
            kp.public_key_pem().unwrap(),
            restored.public_key_pem().unwrap()
        );
    }

    #[test]
    fn test_sign_and_verify() {
        let kp = KeyPair::generate().unwrap();
        let data = b"hello federation world";

        let sig = kp.sign(data).unwrap();
        let pem = kp.public_key_pem().unwrap();

        assert!(KeyPair::verify(&pem, data, &sig).unwrap());
        assert!(!KeyPair::verify(&pem, b"tampered", &sig).unwrap());
    }

    /// RSA-SHA256 (Pkcs1v15 + sha2 0.11 / rsa 0.10) roundtrip after digest generation bump.
    #[test]
    fn rsa_sha256_sign_verify_roundtrip_digest_gen() {
        let kp = KeyPair::generate().unwrap();
        let pem = kp.public_key_pem().unwrap();
        for msg in [
            b"" as &[u8],
            b"short",
            b"federation activity body with unicode \xE8\x81\x94\xE9\x82\xA6 \x00\xff",
        ] {
            let sig = kp.sign(msg).unwrap();
            assert_eq!(sig.len(), 256, "RSA-2048 PKCS#1 v1.5 signature is 256 bytes");
            assert!(
                KeyPair::verify(&pem, msg, &sig).unwrap(),
                "signature must verify for message"
            );
            // Flip one byte of the signature → must fail.
            let mut bad = sig.clone();
            bad[0] ^= 0x01;
            assert!(!KeyPair::verify(&pem, msg, &bad).unwrap());
        }
    }

    #[test]
    fn wrong_jwt_secret_fails_decrypt() {
        // v1 密文不再依赖 JWT_SECRET，所以这条断言只对 v0 遗留格式成立 ——
        // 它描述的正是重新封装要消除的死局。
        let kp = KeyPair::generate().unwrap();
        let secret = "correct-jwt-secret-long-enough-for-tests";
        let encrypted = encrypt_v0(&kp, secret);
        let public_pem = kp.public_key_pem().unwrap();
        let err = KeyPair::from_encrypted(&public_pem, &encrypted, "wrong-secret").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("AES-GCM decryption failed") || msg.contains("decryption"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn short_encrypted_blob_is_rejected() {
        let kp = KeyPair::generate().unwrap();
        let public_pem = kp.public_key_pem().unwrap();
        // Too short to hold nonce + ciphertext
        let short = BASE64.encode([0u8; 4]);
        let err = KeyPair::from_encrypted(&public_pem, &short, "secret").unwrap_err();
        assert!(format!("{err:#}").contains("too short") || format!("{err}").contains("too short"));
    }

    #[test]
    fn debug_redacts_private_key() {
        let kp = KeyPair::generate().unwrap();
        let dbg = format!("{:?}", kp);
        assert!(dbg.contains("REDACTED"));
        assert!(!dbg.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn encrypt_twice_yields_different_ciphertext() {
        let kp = KeyPair::generate().unwrap();
        let secret = "jwt-secret-for-unit-test-long-enough";
        let e1 = kp.encrypt_private_key().unwrap();
        let e2 = kp.encrypt_private_key().unwrap();
        assert_ne!(e1, e2, "random nonce should diversify ciphertext");
        let pem = kp.public_key_pem().unwrap();
        let r1 = KeyPair::from_encrypted(&pem, &e1, secret).unwrap();
        let r2 = KeyPair::from_encrypted(&pem, &e2, secret).unwrap();
        assert_eq!(r1.public_key_pem().unwrap(), r2.public_key_pem().unwrap());
    }

    #[test]
    fn w175_encrypt_twice_different_ct() {
        let kp = KeyPair::generate().unwrap();
        let secret = "jwt-secret-for-unit-test-long-enough";
        let e1 = kp.encrypt_private_key().unwrap();
        let e2 = kp.encrypt_private_key().unwrap();
        assert_ne!(e1, e2);
        let pem = kp.public_key_pem().unwrap();
        assert_eq!(
            KeyPair::from_encrypted(&pem, &e1, secret)
                .unwrap()
                .public_key_pem()
                .unwrap(),
            KeyPair::from_encrypted(&pem, &e2, secret)
                .unwrap()
                .public_key_pem()
                .unwrap()
        );
    }
}

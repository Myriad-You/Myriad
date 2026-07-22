//! 联邦密钥管理
//!
//! RSA-2048 密钥对的生成、加密存储、加载与轮换。
//! 私钥使用 AES-256-GCM 加密后存储到数据库，密钥由 JWT_SECRET 派生。

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand_core::OsRng;
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
        let mut rng = OsRng;
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

    /// 加密私钥用于数据库存储
    ///
    /// 格式：base64(nonce || ciphertext)
    /// 加密算法：AES-256-GCM
    /// 密钥派生：SHA-256(jwt_secret)
    pub fn encrypt_private_key(&self, jwt_secret: &str) -> Result<String> {
        use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

        let pem = self.private_key_pem()?;
        let aes_key = derive_aes_key(jwt_secret);
        let cipher = Aes256Gcm::new_from_slice(&aes_key).context("Failed to create AES cipher")?;

        let nonce_bytes = rand::random::<[u8; AES_NONCE_LEN]>();
        let nonce = Nonce::from(nonce_bytes);

        let ciphertext = cipher
            .encrypt(&nonce, pem.as_bytes())
            .map_err(|e| anyhow::anyhow!("AES-GCM encryption failed: {}", e))?;

        // nonce || ciphertext → base64
        let mut combined = Vec::with_capacity(AES_NONCE_LEN + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);

        Ok(BASE64.encode(&combined))
    }

    /// 从加密存储恢复密钥对
    pub fn from_encrypted(
        public_key_pem: &str,
        encrypted_private_key: &str,
        jwt_secret: &str,
    ) -> Result<Self> {
        use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

        let public_key = RsaPublicKey::from_public_key_pem(public_key_pem)
            .context("Failed to parse public key PEM")?;

        let combined = BASE64
            .decode(encrypted_private_key)
            .context("Failed to decode base64 encrypted key")?;

        if combined.len() < AES_NONCE_LEN + 1 {
            return Err(anyhow::anyhow!("Encrypted key data too short"));
        }

        let (nonce_bytes, ciphertext) = combined.split_at(AES_NONCE_LEN);
        let nonce_bytes: [u8; AES_NONCE_LEN] =
            nonce_bytes.try_into().context("Invalid AES nonce length")?;
        let aes_key = derive_aes_key(jwt_secret);
        let cipher = Aes256Gcm::new_from_slice(&aes_key).context("Failed to create AES cipher")?;
        let nonce = Nonce::from(nonce_bytes);

        let plaintext = cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("AES-GCM decryption failed: {}", e))?;

        let pem = String::from_utf8(plaintext).context("Decrypted key is not valid UTF-8")?;
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

/// 从 JWT_SECRET 派生 AES-256 密钥
fn derive_aes_key(jwt_secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"myriad-federation-key-encryption:");
    hasher.update(jwt_secret.as_bytes());
    hasher.finalize().into()
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

        let encrypted = kp.encrypt_private_key(secret).unwrap();
        let public_pem = kp.public_key_pem().unwrap();

        let restored = KeyPair::from_encrypted(&public_pem, &encrypted, secret).unwrap();
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

    #[test]
    fn wrong_jwt_secret_fails_decrypt() {
        let kp = KeyPair::generate().unwrap();
        let secret = "correct-jwt-secret-long-enough-for-tests";
        let encrypted = kp.encrypt_private_key(secret).unwrap();
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
        assert!(
            format!("{err:#}").contains("too short") || format!("{err}").contains("too short")
        );
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
        let e1 = kp.encrypt_private_key(secret).unwrap();
        let e2 = kp.encrypt_private_key(secret).unwrap();
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
        let e1 = kp.encrypt_private_key(secret).unwrap();
        let e2 = kp.encrypt_private_key(secret).unwrap();
        assert_ne!(e1, e2);
        let pem = kp.public_key_pem().unwrap();
        assert_eq!(
            KeyPair::from_encrypted(&pem, &e1, secret).unwrap().public_key_pem().unwrap(),
            KeyPair::from_encrypted(&pem, &e2, secret).unwrap().public_key_pem().unwrap()
        );

    }

}


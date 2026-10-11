//! Authenticated, versioned sync files. Adapted from the existing v1 implementation; v2 domain separation.
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng, Payload},
    XChaCha20Poly1305, XNonce,
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub type Key = Zeroizing<[u8; 32]>;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Descriptor {
    pub protocol: u32,
    pub library: String,
    pub salt: [u8; 16],
    pub memory_kib: u32,
    pub iterations: u32,
    pub lanes: u32,
    pub check: Vec<u8>,
}
impl Descriptor {
    pub fn create(password: &str) -> Result<(Self, Key), String> {
        if password.chars().count() < 12 {
            return Err("同步密码至少 12 个字符".into());
        }
        let mut salt = [0; 16];
        OsRng.fill_bytes(&mut salt);
        let mut d = Self {
            protocol: 2,
            library: uuid::Uuid::new_v4().to_string(),
            salt,
            memory_kib: 65536,
            iterations: 3,
            lanes: 1,
            check: vec![],
        };
        let key = d.derive(password)?;
        d.check = seal(&key, &d.library, "password-check", b"PasteLeaf sync v2")?;
        Ok((d, key))
    }
    fn derive(&self, password: &str) -> Result<Key, String> {
        // Fixed v2 parameters also bound work on untrusted descriptors.
        if self.protocol != 2
            || self.memory_kib != 65536
            || self.iterations != 3
            || self.lanes != 1
            || uuid::Uuid::parse_str(&self.library).is_err()
        {
            return Err("同步库版本或密钥参数不支持".into());
        }
        let params = Params::new(self.memory_kib, self.iterations, self.lanes, Some(32))
            .map_err(|e| e.to_string())?;
        let mut key = Zeroizing::new([0; 32]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), &self.salt, key.as_mut())
            .map_err(|e| e.to_string())?;
        Ok(key)
    }
    pub fn unlock(&self, password: &str) -> Result<Key, String> {
        let key = self.derive(password)?;
        self.verify(&key)?;
        Ok(key)
    }
    pub fn verify(&self, key: &Key) -> Result<(), String> {
        if open(key, &self.library, "password-check", &self.check)? != b"PasteLeaf sync v2" {
            return Err("同步密码不正确".into());
        }
        Ok(())
    }
}
pub fn seal(key: &Key, library: &str, object: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut nonce);
    let aad = format!("PasteLeaf/2/{library}/{object}");
    let cipher = XChaCha20Poly1305::new((&**key).into());
    let data = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: bytes,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| "同步加密失败")?;
    let mut out = nonce.to_vec();
    out.extend(data);
    Ok(out)
}
pub fn open(key: &Key, library: &str, object: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 40 {
        return Err("同步文件不完整".into());
    }
    let aad = format!("PasteLeaf/2/{library}/{object}");
    XChaCha20Poly1305::new((&**key).into())
        .decrypt(
            XNonce::from_slice(&bytes[..24]),
            Payload {
                msg: &bytes[24..],
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| "密码错误或同步文件认证失败".into())
}

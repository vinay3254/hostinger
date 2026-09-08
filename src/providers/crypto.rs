use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Result};

pub fn encrypt_token(token: &str, key: &[u8; 32]) -> Result<String> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|e| anyhow!("invalid encryption key: {e}"))?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 96-bit nonce
    let ciphertext = cipher
        .encrypt(&nonce, token.as_bytes())
        .map_err(|e| anyhow!("encryption failure: {e}"))?;

    let mut combined = nonce.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(hex::encode(combined))
}

pub fn decrypt_token(encrypted_hex: &str, key: &[u8; 32]) -> Result<String> {
    let bytes =
        hex::decode(encrypted_hex).map_err(|e| anyhow!("invalid hex for encrypted token: {e}"))?;
    if bytes.len() < 12 {
        return Err(anyhow!("encrypted token payload too short"));
    }
    let (nonce_bytes, ciphertext) = bytes.split_at(12);
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|e| anyhow!("invalid encryption key: {e}"))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let decrypted = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!("decryption failure: {e}"))?;
    String::from_utf8(decrypted).map_err(|e| anyhow!("invalid utf-8: {e}"))
}

pub fn default_encryption_key() -> [u8; 32] {
    if let Ok(val) = std::env::var("PROVIDER_ENCRYPTION_KEY") {
        if let Ok(bytes) = hex::decode(val.trim()) {
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                return arr;
            }
        }
    }
    *b"deploy_platform_32byte_secret_k!"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = default_encryption_key();
        let raw = "gho_supersecretaccesstoken123456789";
        let encrypted = encrypt_token(raw, &key).unwrap();
        assert_ne!(raw, encrypted);
        let decrypted = decrypt_token(&encrypted, &key).unwrap();
        assert_eq!(raw, decrypted);
    }
}

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine;
use chrono::{DateTime, Utc};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use sendrs_core::DeviceIdentity;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const APP_DIR: &str = "sendrs";
const IDENTITY_FILE: &str = "identity.json";
const TRUST_FILE: &str = "trusted_peers.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalIdentity {
    pub identity: DeviceIdentity,
    pub private_key_b64: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedPeer {
    pub peer_id: String,
    pub code_hash: String,
    pub paired_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrustStore {
    pub peers: HashMap<String, PairedPeer>,
}

pub fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(format!(".{APP_DIR}"))
}

pub fn load_or_create_identity(display_name: Option<&str>) -> Result<LocalIdentity> {
    let data_dir = default_data_dir();
    fs::create_dir_all(&data_dir).context("create app data dir")?;
    let path = data_dir.join(IDENTITY_FILE);
    if path.exists() {
        return load_identity(&path);
    }

    let mut csprng = OsRng;
    let signing = SigningKey::generate(&mut csprng);
    let verifying = signing.verifying_key();

    let identity = DeviceIdentity {
        device_id: Uuid::new_v4().to_string(),
        display_name: display_name.unwrap_or("sendrs-device").to_string(),
        public_key: b64(verifying.as_bytes()),
    };

    let local = LocalIdentity {
        identity,
        private_key_b64: b64(&signing.to_bytes()),
        created_at: Utc::now(),
    };

    save_json(&path, &local)?;
    Ok(local)
}

pub fn load_identity(path: impl AsRef<Path>) -> Result<LocalIdentity> {
    let content = fs::read_to_string(path.as_ref()).context("read identity file")?;
    let identity =
        serde_json::from_str::<LocalIdentity>(&content).context("parse identity file")?;
    Ok(identity)
}

pub fn load_trust_store() -> Result<TrustStore> {
    let path = default_data_dir().join(TRUST_FILE);
    if !path.exists() {
        return Ok(TrustStore::default());
    }
    let content = fs::read_to_string(path).context("read trust store")?;
    let store = serde_json::from_str::<TrustStore>(&content).context("parse trust store")?;
    Ok(store)
}

pub fn pair_peer(peer_id: &str, short_code: &str) -> Result<PairedPeer> {
    let data_dir = default_data_dir();
    fs::create_dir_all(&data_dir).context("create app data dir")?;
    let path = data_dir.join(TRUST_FILE);

    let mut store = if path.exists() {
        let content = fs::read_to_string(&path).context("read trust store")?;
        serde_json::from_str::<TrustStore>(&content).context("parse trust store")?
    } else {
        TrustStore::default()
    };

    let paired = PairedPeer {
        peer_id: peer_id.to_string(),
        code_hash: hash_code(short_code),
        paired_at: Utc::now(),
    };

    store.peers.insert(peer_id.to_string(), paired.clone());
    save_json(path, &store)?;
    Ok(paired)
}

pub fn is_peer_paired(peer_id: &str, short_code: &str) -> Result<bool> {
    let store = load_trust_store()?;
    Ok(store
        .peers
        .get(peer_id)
        .map(|peer| peer.code_hash == hash_code(short_code))
        .unwrap_or(false))
}

pub fn short_code_from_public_key(public_key_b64: &str) -> String {
    let digest = blake3::hash(public_key_b64.as_bytes()).to_hex().to_string();
    digest.chars().take(6).collect::<String>().to_uppercase()
}

pub fn verify_peer_key(public_key_b64: &str) -> Result<VerifyingKey> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(public_key_b64)
        .context("decode public key")?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid public key length"))?;
    Ok(VerifyingKey::from_bytes(&arr).context("parse public key")?)
}

fn hash_code(code: &str) -> String {
    blake3::hash(code.as_bytes()).to_hex().to_string()
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn save_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    let body = serde_json::to_string_pretty(value).context("serialize json")?;
    fs::write(path, body).context("write json file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_code_is_stable() {
        let code1 = short_code_from_public_key("AAAABBBB");
        let code2 = short_code_from_public_key("AAAABBBB");
        assert_eq!(code1, code2);
    }
}

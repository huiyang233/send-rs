use std::sync::Arc;

use quinn::{TransportConfig, VarInt};
use sendrs_core::NetworkMode;
use serde::{Deserialize, Serialize};

pub const ALPN_SENDRS_V1: &[u8] = b"sendrs/1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTarget {
    pub peer_id: String,
    pub endpoint: String,
    pub network_mode: NetworkMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuicProfile {
    pub max_bidi_streams: u32,
    pub max_uni_streams: u32,
    pub keep_alive_seconds: u64,
}

impl Default for QuicProfile {
    fn default() -> Self {
        Self {
            max_bidi_streams: 128,
            max_uni_streams: 128,
            keep_alive_seconds: 5,
        }
    }
}

pub fn build_transport_config(profile: &QuicProfile) -> Arc<TransportConfig> {
    let mut cfg = TransportConfig::default();
    cfg.max_concurrent_bidi_streams(VarInt::from_u32(profile.max_bidi_streams));
    cfg.max_concurrent_uni_streams(VarInt::from_u32(profile.max_uni_streams));
    cfg.keep_alive_interval(Some(std::time::Duration::from_secs(
        profile.keep_alive_seconds,
    )));
    Arc::new(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_builds_config() {
        let profile = QuicProfile::default();
        let _cfg = build_transport_config(&profile);
    }
}

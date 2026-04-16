use std::collections::HashMap;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Utc;
use sendrs_core::{DeviceIdentity, PeerInfo};
use serde::{Deserialize, Serialize};

pub const DEFAULT_DISCOVERY_PORT: u16 = 39091;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryBeacon {
    pub peer_id: String,
    pub display_name: String,
    pub listen_port: u16,
    pub public_enabled: bool,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedOfferAnnouncement {
    pub code: String,
    pub owner_peer_id: String,
    pub owner_name: String,
    pub source_name: String,
    pub is_dir: bool,
    pub bytes_total: u64,
    pub public_enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscoverySnapshot {
    pub peers: Vec<PeerInfo>,
    pub offers: Vec<SharedOfferAnnouncement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DiscoveryPacket {
    Beacon { beacon: DiscoveryBeacon },
    ShareOffer { offer: SharedOfferAnnouncement },
}

impl DiscoveryBeacon {
    pub fn from_identity(
        identity: &DeviceIdentity,
        listen_port: u16,
        public_enabled: bool,
    ) -> Self {
        Self {
            peer_id: identity.device_id.clone(),
            display_name: identity.display_name.clone(),
            listen_port,
            public_enabled,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

pub fn discover_peers(beacon: &DiscoveryBeacon, timeout: Duration) -> Result<Vec<PeerInfo>> {
    Ok(discover_snapshot(beacon, timeout)?.peers)
}

pub fn discover_snapshot(beacon: &DiscoveryBeacon, timeout: Duration) -> Result<DiscoverySnapshot> {
    broadcast_beacon(beacon, DEFAULT_DISCOVERY_PORT)?;
    listen_for_snapshot(DEFAULT_DISCOVERY_PORT, timeout)
}

pub fn broadcast_beacon(beacon: &DiscoveryBeacon, port: u16) -> Result<()> {
    let packet = DiscoveryPacket::Beacon {
        beacon: beacon.clone(),
    };
    broadcast_packet(&packet, port)
}

pub fn broadcast_share_offer(offer: &SharedOfferAnnouncement, port: u16) -> Result<()> {
    let packet = DiscoveryPacket::ShareOffer {
        offer: offer.clone(),
    };
    broadcast_packet(&packet, port)
}

pub fn listen_for_peers(port: u16, timeout: Duration) -> Result<Vec<PeerInfo>> {
    Ok(listen_for_snapshot(port, timeout)?.peers)
}

pub fn listen_for_snapshot(port: u16, timeout: Duration) -> Result<DiscoverySnapshot> {
    let socket = UdpSocket::bind(("0.0.0.0", port)).context("bind udp listener")?;
    socket
        .set_read_timeout(Some(Duration::from_millis(250)))
        .context("set read timeout")?;

    let mut peers = HashMap::<String, PeerInfo>::new();
    let mut offers = HashMap::<String, SharedOfferAnnouncement>::new();
    let deadline = Instant::now() + timeout;
    let mut buf = vec![0_u8; 8192];

    while Instant::now() < deadline {
        match socket.recv_from(&mut buf) {
            Ok((n, addr)) => {
                let payload = &buf[..n];
                if let Ok(packet) = serde_json::from_slice::<DiscoveryPacket>(payload) {
                    match packet {
                        DiscoveryPacket::Beacon { beacon } => {
                            let entry =
                                peers
                                    .entry(beacon.peer_id.clone())
                                    .or_insert_with(|| PeerInfo {
                                        peer_id: beacon.peer_id.clone(),
                                        display_name: beacon.display_name.clone(),
                                        addresses: vec![format!(
                                            "{}:{}",
                                            addr.ip(),
                                            beacon.listen_port
                                        )],
                                        public_enabled: beacon.public_enabled,
                                        last_seen: Utc::now(),
                                    });
                            let addr_text = format!("{}:{}", addr.ip(), beacon.listen_port);
                            if !entry.addresses.iter().any(|x| x == &addr_text) {
                                entry.addresses.push(addr_text);
                            }
                            entry.last_seen = Utc::now();
                        }
                        DiscoveryPacket::ShareOffer { offer } => {
                            let key = format!("{}:{}", offer.owner_peer_id, offer.code);
                            offers.insert(key, offer);
                        }
                    }
                } else if let Ok(beacon) = serde_json::from_slice::<DiscoveryBeacon>(payload) {
                    let entry = peers
                        .entry(beacon.peer_id.clone())
                        .or_insert_with(|| PeerInfo {
                            peer_id: beacon.peer_id.clone(),
                            display_name: beacon.display_name.clone(),
                            addresses: vec![format!("{}:{}", addr.ip(), beacon.listen_port)],
                            public_enabled: beacon.public_enabled,
                            last_seen: Utc::now(),
                        });
                    let addr_text = format!("{}:{}", addr.ip(), beacon.listen_port);
                    if !entry.addresses.iter().any(|x| x == &addr_text) {
                        entry.addresses.push(addr_text);
                    }
                    entry.last_seen = Utc::now();
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(err) => return Err(err).context("read discovery datagram"),
        }
    }

    Ok(DiscoverySnapshot {
        peers: peers.into_values().collect(),
        offers: offers.into_values().collect(),
    })
}

fn broadcast_packet(packet: &DiscoveryPacket, port: u16) -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").context("bind udp sender")?;
    socket.set_broadcast(true).context("enable udp broadcast")?;
    let payload = serde_json::to_vec(packet)?;
    socket
        .send_to(&payload, format!("255.255.255.255:{port}"))
        .context("send discovery packet")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beacon_serializes() {
        let beacon = DiscoveryBeacon {
            peer_id: "peer-a".to_string(),
            display_name: "Alice".to_string(),
            listen_port: 38080,
            public_enabled: false,
            version: "0.1.0".to_string(),
        };
        let buf = serde_json::to_vec(&DiscoveryPacket::Beacon {
            beacon: beacon.clone(),
        })
        .unwrap();
        let parsed: DiscoveryPacket = serde_json::from_slice(&buf).unwrap();
        match parsed {
            DiscoveryPacket::Beacon { beacon: got } => assert_eq!(got.peer_id, beacon.peer_id),
            _ => panic!("expected beacon"),
        }
    }

    #[test]
    fn share_offer_serializes() {
        let offer = SharedOfferAnnouncement {
            code: "ABCD-1234".to_string(),
            owner_peer_id: "peer-a".to_string(),
            owner_name: "Alice".to_string(),
            source_name: "movie.mkv".to_string(),
            is_dir: false,
            bytes_total: 123,
            public_enabled: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let buf = serde_json::to_vec(&DiscoveryPacket::ShareOffer {
            offer: offer.clone(),
        })
        .unwrap();
        let parsed: DiscoveryPacket = serde_json::from_slice(&buf).unwrap();
        match parsed {
            DiscoveryPacket::ShareOffer { offer: got } => assert_eq!(got.code, offer.code),
            _ => panic!("expected offer"),
        }
    }
}

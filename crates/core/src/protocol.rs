use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalMessage {
    Register {
        peer_id: String,
    },
    Offer {
        from: String,
        to: String,
        sdp: String,
    },
    Answer {
        from: String,
        to: String,
        sdp: String,
    },
    Candidate {
        from: String,
        to: String,
        candidate: String,
    },
    PunchResult {
        from: String,
        to: String,
        success: bool,
        reason: Option<String>,
    },
    Disconnect {
        from: String,
        to: String,
        reason: Option<String>,
    },
    Error {
        message: String,
    },
}

impl SignalMessage {
    pub fn target_peer(&self) -> Option<&str> {
        match self {
            Self::Offer { to, .. }
            | Self::Answer { to, .. }
            | Self::Candidate { to, .. }
            | Self::PunchResult { to, .. }
            | Self::Disconnect { to, .. } => Some(to),
            _ => None,
        }
    }
}

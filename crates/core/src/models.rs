use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub display_name: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub display_name: String,
    pub addresses: Vec<String>,
    pub public_enabled: bool,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    Lan,
    Public,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferTaskStatus {
    AwaitingAccept,
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferTask {
    pub task_id: String,
    pub peer_id: String,
    pub source_path: String,
    pub target_path: Option<String>,
    pub direction: Direction,
    pub status: TransferTaskStatus,
    pub network_mode: NetworkMode,
    pub bytes_total: u64,
    pub bytes_done: u64,
    #[serde(default)]
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TransferTask {
    pub fn new_send(
        peer_id: String,
        source_path: String,
        network_mode: NetworkMode,
        bytes_total: u64,
    ) -> Self {
        let now = Utc::now();
        Self {
            task_id: Uuid::new_v4().to_string(),
            peer_id,
            source_path,
            target_path: None,
            direction: Direction::Send,
            status: TransferTaskStatus::AwaitingAccept,
            network_mode,
            bytes_total,
            bytes_done: 0,
            error_message: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn update_progress(&mut self, bytes_done: u64) {
        self.bytes_done = bytes_done.min(self.bytes_total);
        if self.bytes_done == self.bytes_total {
            self.status = TransferTaskStatus::Completed;
            self.error_message = None;
        } else if self.bytes_done > 0 {
            self.status = TransferTaskStatus::InProgress;
        }
        self.updated_at = Utc::now();
    }

    pub fn accept_receive(&mut self, target_path: String) {
        self.target_path = Some(target_path);
        self.status = TransferTaskStatus::Pending;
        self.error_message = None;
        self.updated_at = Utc::now();
    }

    pub fn mark_in_progress(&mut self) {
        self.status = TransferTaskStatus::InProgress;
        self.error_message = None;
        self.updated_at = Utc::now();
    }

    pub fn mark_failed(&mut self, message: impl Into<String>) {
        self.status = TransferTaskStatus::Failed;
        self.error_message = Some(message.into());
        self.updated_at = Utc::now();
    }
}

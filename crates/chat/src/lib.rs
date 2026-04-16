use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatDirection {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub peer_id: String,
    pub direction: ChatDirection,
    pub body: String,
    pub sent_at: DateTime<Utc>,
}

pub struct ChatStore {
    conn: Connection,
}

impl ChatStore {
    pub fn open_default() -> Result<Self> {
        let root = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".sendrs");
        std::fs::create_dir_all(&root).context("create data dir")?;
        Self::open(root.join("chat.db"))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).context("open sqlite")?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS chat_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                peer_id TEXT NOT NULL,
                direction TEXT NOT NULL,
                body TEXT NOT NULL,
                sent_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chat_peer_sent_at
            ON chat_messages(peer_id, sent_at DESC);
            ",
        )
        .context("initialize schema")?;
        Ok(Self { conn })
    }

    pub fn append_message(
        &self,
        peer_id: &str,
        direction: ChatDirection,
        body: &str,
    ) -> Result<ChatMessage> {
        let now = Utc::now();
        self.conn
            .execute(
                "INSERT INTO chat_messages(peer_id, direction, body, sent_at) VALUES (?1, ?2, ?3, ?4)",
                params![peer_id, direction_to_str(direction), body, now.timestamp()],
            )
            .context("insert chat message")?;

        let id = self.conn.last_insert_rowid();
        Ok(ChatMessage {
            id,
            peer_id: peer_id.to_string(),
            direction,
            body: body.to_string(),
            sent_at: now,
        })
    }

    pub fn list_messages(&self, peer_id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, peer_id, direction, body, sent_at
                 FROM chat_messages
                 WHERE peer_id = ?1
                 ORDER BY sent_at DESC, id DESC
                 LIMIT ?2",
            )
            .context("prepare query")?;

        let iter = stmt
            .query_map(params![peer_id, limit as i64], |row| {
                let direction_str: String = row.get(2)?;
                let ts: i64 = row.get(4)?;
                Ok(ChatMessage {
                    id: row.get(0)?,
                    peer_id: row.get(1)?,
                    direction: str_to_direction(&direction_str),
                    body: row.get(3)?,
                    sent_at: DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_else(Utc::now),
                })
            })
            .context("query messages")?;

        let mut out = iter
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("collect messages")?;
        out.reverse();
        Ok(out)
    }
}

fn direction_to_str(direction: ChatDirection) -> &'static str {
    match direction {
        ChatDirection::Incoming => "incoming",
        ChatDirection::Outgoing => "outgoing",
    }
}

fn str_to_direction(raw: &str) -> ChatDirection {
    if raw.eq_ignore_ascii_case("incoming") {
        ChatDirection::Incoming
    } else {
        ChatDirection::Outgoing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_list_messages() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = ChatStore::open(tmp.path()).unwrap();
        store
            .append_message("peer-a", ChatDirection::Outgoing, "hello")
            .unwrap();
        store
            .append_message("peer-a", ChatDirection::Incoming, "world")
            .unwrap();

        let messages = store.list_messages("peer-a", 20).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].body, "hello");
        assert_eq!(messages[1].body, "world");
    }
}

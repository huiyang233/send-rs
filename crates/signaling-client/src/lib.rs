use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use sendrs_core::SignalMessage;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, warn};

pub struct SignalingClient {
    outbound: mpsc::UnboundedSender<SignalMessage>,
    inbound: mpsc::UnboundedReceiver<SignalMessage>,
}

impl SignalingClient {
    pub async fn connect(url: &str, peer_id: &str) -> Result<Self> {
        let (ws, _) = connect_async(url).await.context("connect signaling ws")?;
        let (mut writer, mut reader) = ws.split();

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<SignalMessage>();
        let (in_tx, in_rx) = mpsc::unbounded_channel::<SignalMessage>();

        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                match serde_json::to_string(&msg) {
                    Ok(text) => {
                        if writer.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => warn!("failed to encode signal message: {err}"),
                }
            }
            let _ = writer.close().await;
        });

        tokio::spawn(async move {
            while let Some(incoming) = reader.next().await {
                match incoming {
                    Ok(Message::Text(text)) => match serde_json::from_str::<SignalMessage>(&text) {
                        Ok(msg) => {
                            if in_tx.send(msg).is_err() {
                                break;
                            }
                        }
                        Err(err) => warn!("bad signal payload: {err}"),
                    },
                    Ok(Message::Binary(bytes)) => {
                        match serde_json::from_slice::<SignalMessage>(&bytes) {
                            Ok(msg) => {
                                if in_tx.send(msg).is_err() {
                                    break;
                                }
                            }
                            Err(err) => warn!("bad signal payload (binary): {err}"),
                        }
                    }
                    Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(err) => {
                        warn!("signaling stream error: {err}");
                        break;
                    }
                }
            }
            debug!("signaling reader terminated");
        });

        let client = Self {
            outbound: out_tx,
            inbound: in_rx,
        };

        client
            .send(SignalMessage::Register {
                peer_id: peer_id.to_string(),
            })
            .context("send register")?;

        Ok(client)
    }

    pub fn send(&self, msg: SignalMessage) -> Result<()> {
        self.outbound
            .send(msg)
            .map_err(|_| anyhow::anyhow!("signaling channel closed"))
    }

    pub async fn recv(&mut self) -> Option<SignalMessage> {
        self.inbound.recv().await
    }
}

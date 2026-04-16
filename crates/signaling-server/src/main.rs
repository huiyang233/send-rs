use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use sendrs_core::SignalMessage;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

#[derive(Clone, Default)]
struct AppState {
    peers: Arc<RwLock<HashMap<String, mpsc::UnboundedSender<Message>>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let state = AppState::default();
    let app = Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:38081".parse()?;
    info!("signaling server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> &'static str {
    "ok"
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();

    let sender_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    let mut registered_peer: Option<String> = None;

    while let Some(incoming) = receiver.next().await {
        let msg = match incoming {
            Ok(msg) => msg,
            Err(err) => {
                warn!("websocket receive error: {err}");
                break;
            }
        };

        let text = match msg {
            Message::Text(text) => text.to_string(),
            Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(_) => break,
        };

        let signal = match serde_json::from_str::<SignalMessage>(&text) {
            Ok(signal) => signal,
            Err(err) => {
                let _ = out_tx.send(Message::Text(
                    serde_json::to_string(&SignalMessage::Error {
                        message: format!("invalid payload: {err}"),
                    })
                    .unwrap_or_else(|_| {
                        "{\"type\":\"error\",\"message\":\"invalid payload\"}".to_string()
                    })
                    .into(),
                ));
                continue;
            }
        };

        match signal {
            SignalMessage::Register { peer_id } => {
                info!("peer registered: {peer_id}");
                state
                    .peers
                    .write()
                    .await
                    .insert(peer_id.clone(), out_tx.clone());
                registered_peer = Some(peer_id);
            }
            other => {
                if let Some(target) = other.target_peer() {
                    if let Some(peer_tx) = state.peers.read().await.get(target).cloned() {
                        let payload = match serde_json::to_string(&other) {
                            Ok(payload) => payload,
                            Err(err) => {
                                warn!("failed to serialize relay message: {err}");
                                continue;
                            }
                        };
                        let _ = peer_tx.send(Message::Text(payload.into()));
                    } else {
                        let _ = out_tx.send(Message::Text(
                            serde_json::to_string(&SignalMessage::Error {
                                message: format!("target peer not connected: {target}"),
                            })
                            .unwrap_or_else(|_| {
                                "{\"type\":\"error\",\"message\":\"target peer not connected\"}"
                                    .to_string()
                            })
                            .into(),
                        ));
                    }
                }
            }
        }
    }

    if let Some(peer_id) = registered_peer {
        state.peers.write().await.remove(&peer_id);
        info!("peer disconnected: {peer_id}");
    }

    sender_task.abort();
}

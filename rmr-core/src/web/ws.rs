use actix_web::{web, HttpRequest, HttpResponse, Error};
use actix_ws::Message;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use std::sync::Arc;
use std::collections::HashSet;
use uuid::Uuid;
use std::hash::{Hash, Hasher};
use crate::web::AppState;

#[derive(Clone)]
pub struct SessionTx {
    pub id: String,
    pub tx: mpsc::Sender<String>,
}

impl PartialEq for SessionTx {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for SessionTx {}

impl Hash for SessionTx {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

pub async fn ws_handler(
    req: HttpRequest,
    stream: web::Payload,
    app_state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let (response, mut session, mut msg_stream) = actix_ws::handle(&req, stream)?;

    let id = Uuid::new_v4().to_string();
    let (tx, mut rx) = mpsc::channel(100);

    let session_tx = SessionTx { id: id.clone(), tx };
    app_state.ws_clients.write().await.insert(session_tx.clone());

    let clients = app_state.ws_clients.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(msg) = rx.recv() => {
                    if session.text(msg).await.is_err() {
                        break;
                    }
                }
                Some(Ok(msg)) = msg_stream.recv() => {
                    match msg {
                        Message::Ping(bytes) => {
                            if session.pong(&bytes).await.is_err() { break; }
                        }
                        Message::Close(_) => break,
                        _ => {}
                    }
                }
                else => break,
            }
        }
        clients.write().await.remove(&session_tx);
    });

    Ok(response)
}

pub async fn broadcast_status(clients: Arc<RwLock<HashSet<SessionTx>>>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        let status = serde_json::json!({"event": "printer_status", "data": {"state": "printing", "progress": 0.42}});
        let msg = status.to_string();
        let clients_guard = clients.read().await;
        for client in clients_guard.iter() {
            let _ = client.tx.send(msg.clone()).await;
        }
    }
}
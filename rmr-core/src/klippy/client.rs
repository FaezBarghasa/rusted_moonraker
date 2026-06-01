use std::path::PathBuf;
use std::collections::HashMap;
use std::time::Duration;
use tokio::net::UnixStream;
use tokio::sync::{mpsc, watch, oneshot};
use tokio_util::codec::Framed;
use futures_util::{StreamExt, SinkExt};
use serde_json::Value;

use crate::klippy::codec::KlippyUdsCodec;
use crate::klippy::state::KlipperStateTree;

#[derive(Debug, thiserror::Error)]
pub enum KlippyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Connection lost")]
    ConnectionLost,

    #[error("Command failed: {0}")]
    CommandFailed(String),
}

pub enum KlippyCommand {
    EmergencyStop,
    SetTargetTemperature(f64),
    JsonRpcRequest {
        method: String,
        params: Value,
        response_sender: oneshot::Sender<Result<Value, KlippyError>>,
    },
}

pub struct KlippyConnectionActor {
    uds_path: PathBuf,
    command_receiver: mpsc::Receiver<KlippyCommand>,
    state_sender: watch::Sender<KlipperStateTree>,
}

impl KlippyConnectionActor {
    pub fn spawn(
        uds_path: PathBuf,
        command_receiver: mpsc::Receiver<KlippyCommand>,
    ) -> (Self, watch::Receiver<KlipperStateTree>) {
        let (state_sender, state_receiver) = watch::channel(KlipperStateTree::default());
        let actor = KlippyConnectionActor {
            uds_path,
            command_receiver,
            state_sender,
        };
        (actor, state_receiver)
    }

    pub async fn run_actor_loop(mut self) -> Result<(), KlippyError> {
        let mut backoff = 1;
        loop {
            match UnixStream::connect(&self.uds_path).await {
                Ok(stream) => {
                    backoff = 1;
                    if let Err(e) = self.handle_connection(stream).await {
                        eprintln!("Klippy connection error: {:?}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to connect to Klippy UDS, retrying in {}s: {:?}", backoff, e);
                }
            }
            tokio::time::sleep(Duration::from_secs(backoff)).await;
            backoff = std::cmp::min(backoff * 2, 32);
        }
    }

    async fn handle_connection(&mut self, stream: UnixStream) -> Result<(), KlippyError> {
        let mut framed = Framed::new(stream, KlippyUdsCodec);
        
        // Dispatch identify handshake
        let identify = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "emulator_identify",
            "id": 1
        });
        framed.send(identify).await?;

        let mut pending_requests: HashMap<u64, oneshot::Sender<Result<Value, KlippyError>>> = HashMap::new();
        let mut next_id = 2u64;

        loop {
            tokio::select! {
                cmd_opt = self.command_receiver.recv() => {
                    match cmd_opt {
                        Some(KlippyCommand::EmergencyStop) => {
                            let payload = serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": "printer.emergency_stop",
                                "id": next_id
                            });
                            next_id += 1;
                            framed.send(payload).await?;
                        }
                        Some(KlippyCommand::SetTargetTemperature(temp)) => {
                            let payload = serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": "printer.gcode.script",
                                "params": {
                                    "script": format!("M104 S{}", temp)
                                },
                                "id": next_id
                            });
                            next_id += 1;
                            framed.send(payload).await?;
                        }
                        Some(KlippyCommand::JsonRpcRequest { method, params, response_sender }) => {
                            let id = next_id;
                            next_id += 1;
                            let payload = serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": method,
                                "params": params,
                                "id": id
                            });
                            pending_requests.insert(id, response_sender);
                            framed.send(payload).await?;
                        }
                        None => {
                            // Command receiver closed, terminate actor
                            return Ok(());
                        }
                    }
                }
                msg_opt = framed.next() => {
                    match msg_opt {
                        Some(Ok(value)) => {
                            self.process_incoming_message(value, &mut pending_requests);
                        }
                        Some(Err(e)) => {
                            return Err(KlippyError::Io(e));
                        }
                        None => {
                            return Err(KlippyError::ConnectionLost);
                        }
                    }
                }
            }
        }
    }

    fn process_incoming_message(
        &self,
        value: Value,
        pending_requests: &mut HashMap<u64, oneshot::Sender<Result<Value, KlippyError>>>
    ) {
        if let Some(id_val) = value.get("id") {
            if let Some(id) = id_val.as_u64() {
                if let Some(sender) = pending_requests.remove(&id) {
                    if let Some(err) = value.get("error") {
                        let _ = sender.send(Err(KlippyError::CommandFailed(err.to_string())));
                    } else {
                        let result = value.get("result").cloned().unwrap_or(Value::Null);
                        let _ = sender.send(Ok(result));
                    }
                }
            }
        } else if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
            // Notification updates from Klipper
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            let mut current_state = self.state_sender.borrow().clone();
            if update_state_from_notification(&mut current_state, method, &params) {
                let _ = self.state_sender.send(current_state);
            }
        }
    }
}

fn update_state_from_notification(state: &mut KlipperStateTree, method: &str, params: &Value) -> bool {
    if method == "notify_status_update" {
        if let Some(arr) = params.as_array() {
            if let Some(obj) = arr.first() {
                let mut changed = false;
                if let Some(extruder) = obj.get("extruder") {
                    if let Some(temp) = extruder.get("temperature").and_then(|v| v.as_f64()) {
                        state.tool_temp = temp;
                        changed = true;
                    }
                    if let Some(target) = extruder.get("target").and_then(|v| v.as_f64()) {
                        state.target_temp = target;
                        changed = true;
                    }
                }
                if let Some(toolhead) = obj.get("toolhead") {
                    if let Some(pos) = toolhead.get("position").and_then(|v| v.as_array()) {
                        if pos.len() >= 4 {
                            state.x = pos[0].as_f64().unwrap_or(state.x);
                            state.y = pos[1].as_f64().unwrap_or(state.y);
                            state.z = pos[2].as_f64().unwrap_or(state.z);
                            state.e = pos[3].as_f64().unwrap_or(state.e);
                            changed = true;
                        }
                    }
                }
                if let Some(print_stats) = obj.get("print_stats") {
                    if let Some(state_str) = print_stats.get("state").and_then(|v| v.as_str()) {
                        state.print_status = match state_str.to_lowercase().as_str() {
                            "printing" => "Printing".to_string(),
                            "paused" => "Paused".to_string(),
                            _ => "Idle".to_string(),
                        };
                        changed = true;
                    }
                    if let Some(prog) = print_stats.get("progress").and_then(|v| v.as_f64()) {
                        state.print_progress = prog;
                        changed = true;
                    }
                }
                return changed;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;
    use std::time::SystemTime;

    fn get_temp_socket_path() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        PathBuf::from(format!("/tmp/mock_uds_{}.sock", now))
    }

    #[tokio::test]
    async fn test_uds_handshake_and_reconnect() {
        let socket_path = get_temp_socket_path();
        let _ = std::fs::remove_file(&socket_path);

        // Spawn mock UDS listener
        let listener = UnixListener::bind(&socket_path).unwrap();

        let (cmd_tx, cmd_rx) = mpsc::channel(10);
        let (actor, _state_rx) = KlippyConnectionActor::spawn(socket_path.clone(), cmd_rx);

        // Spawn actor loop
        let actor_handle = tokio::spawn(actor.run_actor_loop());

        // Wait for connection on mock server
        let (stream, _) = listener.accept().await.unwrap();
        let mut framed_srv = Framed::new(stream, KlippyUdsCodec);

        // Read handshake
        let handshake = framed_srv.next().await.unwrap().unwrap();
        assert_eq!(handshake["method"], "emulator_identify");

        // Send identify result back
        framed_srv.send(serde_json::json!({
            "jsonrpc": "2.0",
            "result": "ok",
            "id": 1
        })).await.unwrap();

        // Send a custom request from client to server
        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx.send(KlippyCommand::JsonRpcRequest {
            method: "printer.info".to_string(),
            params: serde_json::json!({}),
            response_sender: resp_tx,
        }).await.unwrap();

        // Server receives it
        let req = framed_srv.next().await.unwrap().unwrap();
        assert_eq!(req["method"], "printer.info");
        let req_id = req["id"].as_u64().unwrap();

        // Server sends response
        framed_srv.send(serde_json::json!({
            "jsonrpc": "2.0",
            "result": {"state": "ready"},
            "id": req_id
        })).await.unwrap();

        // Client gets response
        let resp = resp_rx.await.unwrap().unwrap();
        assert_eq!(resp["state"], "ready");

        // Now test reconnection: close stream
        drop(framed_srv);

        // Spawn a new listener socket (the actor will reconnect)
        // Wait for reconnect and handshake
        let (stream2, _) = listener.accept().await.unwrap();
        let mut framed_srv2 = Framed::new(stream2, KlippyUdsCodec);

        let handshake2 = framed_srv2.next().await.unwrap().unwrap();
        assert_eq!(handshake2["method"], "emulator_identify");

        // Clean up
        actor_handle.abort();
        let _ = std::fs::remove_file(&socket_path);
    }
}

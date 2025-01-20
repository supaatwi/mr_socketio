use futures_util::{SinkExt, StreamExt};
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;

use crate::error::SocketIOError;

type EventCallback = Arc<dyn Fn(Value) -> () + Send + Sync>;
type EventHandlers = Arc<Mutex<HashMap<String, Vec<EventCallback>>>>;

#[derive(Debug, Deserialize, Serialize)]
struct SocketIOHandshake {
    sid: String,
    upgrades: Vec<String>,
    #[serde(rename = "pingInterval")]
    ping_interval: u64,
    #[serde(rename = "pingTimeout")]
    ping_timeout: u64,
}

#[derive(Clone)]
pub struct SocketIOClient {
    base_url: String,
    namespace: String,
    session_id: Arc<Mutex<Option<String>>>,
    http_client: reqwest::Client,
    writer: Arc<Mutex<Option<Arc<Mutex<futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
        >,
        Message
    >>>>>>,
    event_handlers: EventHandlers,
    is_connected: Arc<Mutex<bool>>,
    rooms: Arc<Mutex<Vec<String>>>,
}

impl SocketIOClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            namespace: "/".to_string(), // Default namespace
            session_id: Arc::new(Mutex::new(None)),
            http_client: reqwest::Client::new(),
            writer: Arc::new(Mutex::new(None)),
            is_connected: Arc::new(Mutex::new(false)),
            event_handlers: Arc::new(Mutex::new(HashMap::new())),
            rooms: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_namespace(base_url: &str, namespace: &str) -> Self {
        let mut client = Self::new(base_url);
        client.namespace = namespace.to_string();
        client
    }

    pub async fn join_room(&self, room: &str) -> Result<(), SocketIOError> {
        {
            let mut rooms = self.rooms.lock().await;
            if !rooms.contains(&room.to_string()) {
                rooms.push(room.to_string());
            }
        }
        self.emit("join", json!(room)).await
    }

    pub async fn leave_room(&self, room: &str) -> Result<(), SocketIOError> {
        {
            let mut rooms = self.rooms.lock().await;
            rooms.retain(|r| r != room);
        }
        self.emit("leave", json!(room)).await
    }

    pub async fn get_rooms(&self) -> Vec<String> {
        self.rooms.lock().await.clone()
    }

    pub async fn on<F>(&self, event: &str, callback: F)
    where
        F: Fn(Value) -> () + Send + Sync + 'static,
    {
        let mut handlers = self.event_handlers.lock().await;
        let event_handlers = handlers
            .entry(event.to_string())
            .or_insert_with(Vec::new);
        event_handlers.push(Arc::new(callback));
    }

    pub async fn connect(&self) -> Result<(), SocketIOError> {
        let handshake_url = format!(
            "{}/socket.io/?EIO=4&transport=polling&namespace={}",
            self.base_url, self.namespace
        );
        let response = self.http_client.get(&handshake_url).send().await?.text().await?;

        let json_start = response
            .find('{')
            .ok_or(SocketIOError::HandshakeFailed("Invalid handshake response".into()))?;
        let handshake: SocketIOHandshake = serde_json::from_str(&response[json_start..])?;

        {
            let mut session_id = self.session_id.lock().await;
            *session_id = Some(handshake.sid.clone());
        }
        println!("Got session ID: {}", handshake.sid);

        let ws_url = format!(
            "ws{}://{}/socket.io/?EIO=4&transport=websocket&sid={}&namespace={}",
            if self.base_url.starts_with("https") { "s" } else { "" },
            self.base_url.trim_start_matches("http://").trim_start_matches("https://"),
            handshake.sid,
            self.namespace
        );

        let (ws_stream, _) = connect_async(Url::parse(&ws_url)?).await?;
        let (write, mut read) = ws_stream.split();
        
        {
            let mut writer_guard = self.writer.lock().await;
            *writer_guard = Some(Arc::new(Mutex::new(write)));
        }

        let event_handlers = self.event_handlers.clone();

        // Send initial probe
        self.emit_raw("2probe").await?;
        
        let mut upgraded = false;
        while let Some(msg) = read.next().await {
            match msg? {
                Message::Text(text) => {
                    if text == "3probe" {
                        // Send connect packet for the namespace
                        if self.namespace != "/" {
                            self.emit_raw(&format!("40{}", self.namespace)).await?;
                        }
                        self.emit_raw("5").await?;
                        upgraded = true;
                        {
                            let mut is_connected_guard = self.is_connected.lock().await;
                            *is_connected_guard = true;
                        }
                    } else if upgraded {
                        if text.starts_with("42") {
                            if let Ok(array) = serde_json::from_str::<Vec<Value>>(&text[2..]) {
                                if let (Some(event), Some(data)) = (array.get(0), array.get(1)) {
                                    if let Some(event_str) = event.as_str() {
                                        let handlers = event_handlers.lock().await;
                                        if let Some(callbacks) = handlers.get(event_str) {
                                            for callback in callbacks {
                                                callback(data.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        } else if text.starts_with("40") {
                            // Namespace connected
                            println!("Connected to namespace: {}", self.namespace);
                            // Rejoin rooms if any
                            let rooms = self.rooms.lock().await.clone();
                            for room in rooms {
                                self.join_room(&room).await?;
                            }
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }

        Ok(())
    }

    async fn emit_raw(&self, message: &str) -> Result<(), SocketIOError> {
        if let Some(writer) = self.writer.lock().await.as_ref() {
            let mut writer = writer.lock().await;
            writer.send(Message::Text(message.to_string())).await?;
        }
        Ok(())
    }
    
    pub async fn emit(&self, event: &str, data: Value) -> Result<(), SocketIOError> {
        let message = json!([event, data]);
        let packet = format!("42{}", message.to_string());
        self.emit_raw(&packet).await
    }

    pub async fn emit_to_room(&self, room: &str, event: &str, data: Value) -> Result<(), SocketIOError> {
        let message = json!({
            "room": room,
            "event": event,
            "data": data
        });
        self.emit("room_message", message).await
    }

    pub async fn session_id(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    pub async fn is_connected(&self) -> bool {
        self.is_connected.lock().await.clone()
    }

    pub fn get_namespace(&self) -> String {
        self.namespace.clone()
    }
}
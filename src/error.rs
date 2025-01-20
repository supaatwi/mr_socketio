use reqwest;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SocketIOError {
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("WebSocket error: {0}")]
    WebSocketError(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("URL parse error: {0}")]
    UrlError(#[from] url::ParseError),
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),
}

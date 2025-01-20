use std::sync::Arc;
use mr_socketio::client::SocketIOClient;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(SocketIOClient::new("http://localhost:4000"));

    println!("Starting connection...");
    client.on("chat", |data| {
        println!("Received chat event: {:?}", data);
    }).await;

    // Connect in a separate task
    let connect_handle = {
        let client = client.clone();
        tokio::spawn(async move {
            client.connect().await
        })
    };


    println!("Wait a bit for the connection to establish");
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        if client.is_connected().await {
            break;
        }
    }

    // Emit the event
    println!("Emitting chat event...");
    client.emit("chat", json!({
        "key": "Say Hello",
    })).await?;


    // Wait for the connection task to complete
    connect_handle.await??;

    Ok(())
}
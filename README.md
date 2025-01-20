# Mr socketio

## Example usage (Client)

Add the following to your `Cargo.toml` file:

```rust
mr_socketio = "*"
```

Then you're able to run the following example code:

```rust
use std::sync::Arc;
use mr_socketio::client::SocketIOClient;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
 
let client = Arc::new(SocketIOClient::new("host"));

    println!("Starting connection...");
    client.on("chat", |data| {
        println!("Received message event: {:?}", data);
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
        "say": "Hello Socket Client"
    })).await?;


    // Wait for the connection task to complete
    connect_handle.await??;

    Ok(())
}

```

## Licence

MIT

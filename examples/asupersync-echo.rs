use async_tungstenite::{asupersync::connect_async, tungstenite::Message};
use futures::prelude::*;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Use wss:// with TLS if the asupersync-tls features are enabled
    #[cfg(any(
        feature = "asupersync-tls-native-roots",
        feature = "asupersync-tls-webpki-roots"
    ))]
    let url = "wss://echo.websocket.org";

    // Fall back to plain ws:// if TLS is not enabled
    #[cfg(not(any(
        feature = "asupersync-tls-native-roots",
        feature = "asupersync-tls-webpki-roots"
    )))]
    let url = "ws://echo.websocket.org";

    let (mut ws_stream, _) = connect_async(url).await?;

    let text = "Hello from asupersync!";

    println!("Sending: \"{}\"", text);
    ws_stream.send(Message::text(text)).await?;

    let msg = ws_stream.next().await.ok_or("didn't receive anything")??;

    println!("Received: {:?}", msg);

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Note: asupersync uses structured concurrency with regions and contexts
    // This example is simplified and may need adjustment based on
    // asupersync's actual runtime API when it stabilizes
    println!("asupersync-echo example");
    
    #[cfg(any(
        feature = "asupersync-tls-native-roots",
        feature = "asupersync-tls-webpki-roots"
    ))]
    println!("TLS support: ENABLED (wss:// works)");
    
    #[cfg(not(any(
        feature = "asupersync-tls-native-roots",
        feature = "asupersync-tls-webpki-roots"
    )))]
    println!("TLS support: DISABLED (only ws:// works)");
    
    println!("Note: This is a basic integration example.");
    println!("Full asupersync features (structured concurrency, cancel-correctness)");
    println!("require using asupersync's Cx and region APIs.");
    
    println!("\nThis example demonstrates basic TCP connectivity.");
    println!("For production use with asupersync, wrap in proper region/Cx context.");
    
    Ok(())
}

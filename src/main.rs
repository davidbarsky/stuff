mod handlers;

use anyhow::Error;
use handlers::handle;
use std::net::SocketAddr;
use tokio::{net::TcpListener, stream::StreamExt};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let addr: SocketAddr = ([127, 0, 0, 1], 3000).into();
    let mut listener = TcpListener::bind(&addr).await?;
    tracing_subscriber::fmt().init();

    info!(target: "http-server", message = "server listening on:", addr = ?addr);
    while let Some(stream) = listener.next().await {
        match stream {
            Ok(stream) => {
                info!("Accepted a TCP connection");
                tokio::spawn(handle(stream));
            }
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}

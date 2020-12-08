mod duplex;

use anyhow::Error;
use duplex::{chan, SimulatedConnector};
use http::{Method, Request, Response};
use hyper::{server::conn::Http, service::service_fn, Body};
use partial_io::{PartialAsyncWrite, PartialOp};
use std::net::SocketAddr;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    stream::StreamExt,
};
use tower::Service;
use tracing::{error, info, instrument};

async fn hello(req: Request<Body>) -> Result<Response<Body>, Error> {
    Ok(Response::new(Body::from("hello")))
}

async fn handle<I>(io: I) -> Result<(), hyper::error::Error>
where
    I: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let conn = Http::new().serve_connection(io, service_fn(hello));
    if let Err(e) = conn.await {
        error!(message = "Got error serving connection", e = %e);
        return Err(e);
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let addr: SocketAddr = ([127, 0, 0, 1], 3000).into();
    let mut listener = TcpListener::bind(&addr).await?;
    info!(target: "http-server", message = "server listening on:", addr = ?addr);

    while let Some(stream) = listener.incoming().next().await {
        match stream {
            Ok(stream) => tokio::spawn(handle(stream)).await??,
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_stream_handler() -> Result<(), Error> {
    let (client, server) = chan();
    let ops = vec![PartialOp::Err(std::io::ErrorKind::Interrupted)];
    let client = PartialAsyncWrite::new(client, ops);

    let req = Request::builder()
        .method(Method::GET)
        .uri("http://httpbin.org")
        .body(Body::empty())
        .expect("Can't build request");

    let conn = SimulatedConnector { inner: client };
    let mut client = hyper::Client::builder().build(conn);

    tokio::spawn(async {
        handle(server).await.expect("Unable to handle request");
    });

    let res = client.call(req).await.expect("Unable to send request");
    assert_eq!(res.status(), http::StatusCode::OK);

    Ok(())
}

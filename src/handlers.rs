use anyhow::Error;
use http::{Request, Response};
#[cfg(test)]
use hyper::client::conn as client;
use hyper::{server::conn as server, service::service_fn, Body};
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{error, info};

async fn hello(_: Request<Body>) -> Result<Response<Body>, Error> {
    Ok(Response::new(Body::from("hello")))
}

pub async fn handle<I>(io: I) -> Result<(), hyper::Error>
where
    I: AsyncRead + AsyncWrite + Unpin + 'static,
{
    info!("accepted an http connection");
    let conn = server::Http::new().serve_connection(io, service_fn(hello));
    if let Err(e) = conn.await {
        error!(message = "Got error serving connection", e = %e);
        return Err(e);
    }
    Ok(())
}

#[tokio::test]
async fn test_stream_handler() -> Result<(), Error> {
    tracing_subscriber::fmt().with_test_writer().init();
    let (client, server) = tokio::io::duplex(10);

    info!("starting client");
    let (mut tx, conn) = client::Builder::new().handshake::<_, Body>(client).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            error!("Error in connection: {}", e);
        }
    });

    let req = Request::builder()
        .method(http::Method::GET)
        .uri("http://httpbin.org")
        .header("Host", "http://httpbin.org")
        .body(Body::empty())
        .expect("Can't build request");

    tokio::spawn(async {
        handle(server).await.expect("Unable to handle request");
    });

    info!("sent request");
    let res = tx.send_request(req).await?;
    assert_eq!(res.status(), http::StatusCode::OK);

    Ok(())
}

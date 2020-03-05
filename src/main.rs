mod sim_stream;
use anyhow::Error;
use http::{Request, Response};
use hyper::{server::conn::Http, service::service_fn, Body};
use std::net::SocketAddr;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    stream::StreamExt,
};
use tracing::{error, info, instrument};
use tracing_subscriber::{fmt, layer::SubscriberExt, registry::Registry};

macro_rules! constraint {
    (@as_items $($it:item)*) => ($($it)*);
    (@replace_self with $rep:tt [$($st:tt)*] Self $($tail:tt)*) => {
        constraint!{@replace_self with $rep [$($st)* $rep] $($tail)*}
    };
    (@replace_self with $rep:tt [$($st:tt)*] $t:tt $($tail:tt)*) => {
        constraint!{@replace_self with $rep [$($st)* $t] $($tail)*}
    };
    (@replace_self with $rep:tt [$($st:tt)*]) => {
        constraint!{@as_items $($st)*}
    };
    // User-facing rule: pub trait
    ($(#[$attr:meta])* pub trait $name:ident : $($t:tt)+) => {
        constraint!{@as_items $(#[$attr])* pub trait $name : $($t)+ { }}
        constraint!{@replace_self with T [] impl<T> $name for T where T: $($t)+ { }}
    };
    // User-facing rule: (not pub) trait
    ($(#[$attr:meta])* trait $name:ident : $($t:tt)+) => {
        constraint!{@as_items $(#[$attr])* trait $name : $($t)+ { }}
        constraint!{@replace_self with T [] impl<T> $name for T where T: $($t)+ { }}
    };
}

constraint! {
    trait IO: AsyncRead + AsyncWrite + Unpin + 'static
}

#[instrument]
async fn hello(req: Request<Body>) -> Result<Response<Body>, Error> {
    Ok(Response::new(Body::from("hello")))
}

#[instrument(skip(io))]
async fn handle<I: IO>(io: I) -> Result<(), hyper::error::Error> {
    let conn = Http::new().serve_connection(io, service_fn(hello));
    if let Err(e) = conn.await {
        error!(message = "Got error serving connection", e = %e);
        return Err(e);
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let fmt_layer = fmt::Layer::builder().finish();
    tracing::subscriber::set_global_default(Registry::default().with(fmt_layer))?;

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

use hyper::client::connect::Connection;

impl Connection for sim_stream::SimStream {
    fn connected(&self) -> hyper::client::connect::Connected {
        hyper::client::connect::Connected::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        handle,
        sim_stream::{self, chan},
    };
    use anyhow::Error;
    use http::{Method, Request, Uri};
    use hyper::{client::conn::handshake, Body};
    use std::{
        future::Future,
        net::SocketAddr,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::net::TcpStream;
    use tower::{service_fn, Service};
    use tracing_subscriber::{fmt, layer::SubscriberExt, Registry};

    #[derive(Clone)]
    struct LocalConnector {
        inner: sim_stream::SimStream,
    }

    impl Service<Uri> for LocalConnector {
        type Response = sim_stream::SimStream;
        type Error = std::io::Error;
        // We can't "name" an `async` generated future.
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            // This connector is always ready, but others might not be.
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: Uri) -> Self::Future {
            let inner = self.inner.clone();
            Box::pin(async move { Ok(inner) })
        }
    }

    #[tokio::test]
    async fn test_stream_handler() -> Result<(), Error> {
        let subscriber = Registry::default().with(fmt::Layer::default());
        let _s = tracing::subscriber::set_default(subscriber);
        let (client, server) = chan();

        let req = Request::builder()
            .version(http::Version::HTTP_2)
            .method(Method::GET)
            .body(Body::empty())
            .expect("Can't build request");

        // let (mut svc, conn) = handshake(client).await?;
        // tokio::spawn(conn);

        let mut client = hyper::Client::builder().build(LocalConnector { inner: client });

        tokio::spawn(async {
            handle(server).await.expect("Unable to handle request");
        });

        let res = client.call(req).await.expect("Unable to send request");
        assert_eq!(res.status(), http::StatusCode::OK);

        Ok(())
    }
}

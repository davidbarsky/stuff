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

#[cfg(test)]
mod tests {
    use super::{
        handle,
        sim_stream::{chan, SimStream, SimulatedConnector},
    };
    use anyhow::Error;
    use http::{Method, Request};
    use hyper::Body;
    use tower::{service_fn, Service};
    use tracing_subscriber::{fmt, layer::SubscriberExt, Registry};

    #[tokio::test]
    async fn test_stream_handler() -> Result<(), Error> {
        let subscriber = Registry::default().with(fmt::Layer::default());
        let f = tracing::subscriber::with_default(subscriber, || async {
            let (client, server) = chan();

            let req = Request::builder()
                .method(Method::GET)
                .uri("http://httpbin.org")
                .body(Body::empty())
                .expect("Can't build request");

            let conn = service_fn(move |_| {
                let f = Ok::<_, Error>(client.clone());
                Box::pin(async { f })
            });

            // let conn = SimulatedConnector { inner: client };
            let mut client = hyper::Client::builder().build(conn);

            tokio::spawn(async {
                handle(server).await.expect("Unable to handle request");
            });

            let res = client.call(req).await.expect("Unable to send request");
            assert_eq!(res.status(), http::StatusCode::OK);

            Ok(())
        });
        f.await
    }
}

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
    info!(message = "got req", req.headers = ?req.headers());
    Ok(Response::new(Body::from("hello")))
}

#[instrument(skip(io))]
async fn handle<I: IO>(io: I) -> Result<(), hyper::error::Error> {
    info!(message = "got connection");
    let conn = Http::new().serve_connection(io, service_fn(hello));
    info!(message = "serving connection");
    if let Err(e) = conn.await {
        error!(message = "Got error serving connection", e = %e);
        return Err(e);
    }
    info!("serving connection");
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
    use super::handle;
    use anyhow::Error;
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::{
        fs::File,
        io::{AsyncRead, AsyncReadExt, AsyncWrite},
    };
    use tokio_test::io::Builder;
    use tracing_subscriber::{fmt, layer::SubscriberExt, Registry};

    #[tokio::test]
    async fn test_handler() -> Result<(), Error> {
        let fmt_layer = fmt::Layer::builder().finish();
        let _s = tracing::subscriber::set_default(Registry::default().with(fmt_layer));

        let mut req = File::open("fixtures/request.txt")
            .await
            .expect("Unable to open file");
        let mut req_buf = vec![];
        req.read_to_end(&mut req_buf)
            .await
            .expect("Unable to read file into buffer");

        let mut rsp = File::open("fixtures/response.txt")
            .await
            .expect("Unable to open file");
        let mut rsp_buf = vec![];
        rsp.read_to_end(&mut rsp_buf)
            .await
            .expect("Unable to read file into buffer");

        let mock = Builder::new().read(&req_buf).write(&rsp_buf).build();
        handle(mock).await.expect("Unable to handle request");

        Ok(())
    }

    #[derive(Debug, PartialEq)]
    struct MockIO {
        inner: Vec<u8>,
    }

    impl AsyncRead for MockIO {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<Result<usize, io::Error>> {
            let inner: &mut Vec<u8> = self.inner.as_mut();
            let mut inner = io::Cursor::new(inner);
            Poll::Ready(io::Read::read(&mut inner, buf))
        }
    }

    impl AsyncWrite for MockIO {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, io::Error>> {
            let mut inner: &mut Vec<u8> = self.inner.as_mut();
            inner.clear();
            inner.reserve_exact(buf.len());

            Poll::Ready(io::Write::write(&mut inner, buf))
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            Poll::Ready(Ok(()))
        }
    }
}

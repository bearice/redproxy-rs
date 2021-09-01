use async_trait::async_trait;
use easy_error::{Error, ResultExt};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::Sender;

use crate::common::h11c::h11c_handshake;
use crate::common::tls::TlsServerConfig;
use crate::context::{make_buffered_stream, Context, ContextRef};
use crate::listeners::Listener;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HttpListener {
    name: String,
    bind: String,
    tls: Option<TlsServerConfig>,
}

pub fn from_value(value: &serde_yaml::Value) -> Result<Box<dyn Listener>, Error> {
    let ret: HttpListener = serde_yaml::from_value(value.clone()).context("parse config")?;
    Ok(Box::new(ret))
}

#[async_trait]
impl Listener for HttpListener {
    fn name(&self) -> &str {
        &self.name
    }
    async fn init(&mut self) -> Result<(), Error> {
        if let Some(Err(e)) = self.tls.as_mut().map(TlsServerConfig::init) {
            return Err(e);
        }
        Ok(())
    }
    async fn listen(self: Arc<Self>, queue: Sender<ContextRef>) -> Result<(), Error> {
        info!("{} listening on {}", self.name, self.bind);
        let listener = TcpListener::bind(&self.bind).await.context("bind")?;
        let this = self.clone();
        tokio::spawn(this.accept(listener, queue));
        Ok(())
    }
}
impl HttpListener {
    async fn accept(self: Arc<Self>, listener: TcpListener, queue: Sender<ContextRef>) {
        loop {
            match listener.accept().await.context("accept") {
                Ok((socket, source)) => {
                    // we spawn a new thread here to avoid handshake to block accept thread
                    let this = self.clone();
                    let queue = queue.clone();
                    tokio::spawn(async move {
                        let res = match this.create_context(source, socket).await {
                            Ok(ctx) => h11c_handshake(ctx, queue).await,
                            Err(e) => Err(e),
                        };
                        if let Err(e) = res {
                            warn!(
                                "{}: handshake failed: {}\ncause: {:?}",
                                this.name, e, e.cause
                            );
                        }
                    });
                }
                Err(e) => {
                    error!("{} accept error: {} \ncause: {:?}", self.name, e, e.cause);
                    return;
                }
            }
        }
    }
    async fn create_context(
        &self,
        source: SocketAddr,
        socket: TcpStream,
    ) -> Result<ContextRef, Error> {
        let tls_acceptor = self.tls.as_ref().map(|options| options.acceptor());
        let stream = if let Some(acceptor) = tls_acceptor {
            acceptor
                .accept(socket)
                .await
                .context("tls accept error")
                .map(make_buffered_stream)?
        } else {
            make_buffered_stream(socket)
        };
        let ctx = Context::new(self.name.to_owned(), source);
        ctx.write().await.set_client_stream(stream);
        Ok(ctx)
    }
}

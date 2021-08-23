use async_trait::async_trait;
use easy_error::{bail, err_msg, Error, ResultExt};
use log::{info, trace, warn};
use tokio::io::{AsyncWriteExt, BufStream};
use tokio::net::TcpListener;
use tokio::sync::mpsc::Sender;

use crate::common::http::HttpRequest;
use crate::common::tls::TlsServerConfig;
use crate::context::{Context, IOStream};
use crate::listeners::Listener;
use serde::{Deserialize, Serialize};

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
    async fn init(&mut self) -> Result<(), Error> {
        if let Some(Err(e)) = self.tls.as_mut().map(TlsServerConfig::init) {
            return Err(e);
        }
        Ok(())
    }
    async fn listen(&self, queue: Sender<Context>) -> Result<(), Error> {
        info!("{} listening on {}", self.name, self.bind);
        let listener = TcpListener::bind(&self.bind).await.context("bind")?;
        let self = self.clone();
        tokio::spawn(async move {
            loop {
                let accept = async {
                    let tls_acceptor = self.tls.as_ref().map(|options| options.acceptor());
                    let (socket, source) = listener.accept().await.context("accept")?;
                    let socket: Box<dyn IOStream> = if let Some(acceptor) = tls_acceptor {
                        Box::new(acceptor.accept(socket).await.context("tls accept error")?)
                    } else {
                        Box::new(socket)
                    };
                    trace!("connected from {:?}", source);
                    let mut socket = BufStream::new(socket);
                    let request = HttpRequest::new(&mut socket).await?;
                    if !request.method.eq_ignore_ascii_case("CONNECT") {
                        bail!("Invalid request method: {}", request.method)
                    }
                    let target = request.resource.parse().map_err(|_e| {
                        err_msg(format!(
                            "failed to parse target address: {}",
                            request.resource
                        ))
                    })?;
                    socket
                        .write_all("HTTP/1.1 200 Connection established\r\n\r\n".as_bytes())
                        .await
                        .context("write_all")?;
                    socket.flush().await.context("flush")?;
                    queue
                        .send(Context {
                            socket,
                            target,
                            source,
                            listener: self.name().into(),
                        })
                        .await
                        .context("enqueue")?;
                    Ok::<(), Error>(())
                };
                if let Err(e) = accept.await {
                    warn!("{}: {:?}", e, e.cause);
                }
            }
        });
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

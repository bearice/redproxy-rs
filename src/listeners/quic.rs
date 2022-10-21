use async_trait::async_trait;
use chashmap_async::CHashMap;
use easy_error::{Error, ResultExt};
use futures_util::{StreamExt, TryFutureExt};
use log::{debug, info, warn};
use quinn::{congestion, Endpoint, Incoming, NewConnection};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

use crate::common::h11c::h11c_handshake;
use crate::common::quic::{create_quic_frames, create_quic_server, quic_frames_thread, QuicStream};
use crate::common::tls::TlsServerConfig;
use crate::context::{make_buffered_stream, ContextRef};
use crate::listeners::Listener;
use crate::GlobalState;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct QuicListener {
    name: String,
    bind: SocketAddr,
    tls: TlsServerConfig,
    #[serde(default = "default_bbr")]
    bbr: bool,
}

fn default_bbr() -> bool {
    true
}

pub fn from_value(value: &serde_yaml::Value) -> Result<Box<dyn Listener>, Error> {
    let ret: QuicListener = serde_yaml::from_value(value.clone()).context("parse config")?;
    Ok(Box::new(ret))
}

#[async_trait]
impl Listener for QuicListener {
    fn name(&self) -> &str {
        &self.name
    }
    async fn init(&mut self) -> Result<(), Error> {
        self.tls.init()?;
        Ok(())
    }
    async fn listen(
        self: Arc<Self>,
        state: Arc<GlobalState>,
        queue: Sender<ContextRef>,
    ) -> Result<(), Error> {
        info!("{} listening on {}", self.name, self.bind);
        let mut cfg = create_quic_server(&self.tls)?;
        if self.bbr {
            let transport = Arc::get_mut(&mut cfg.transport).unwrap();
            transport.congestion_controller_factory(Arc::new(congestion::BbrConfig::default()));
        }
        let (endpoint, incoming) = Endpoint::server(cfg, self.bind).context("quic_listen")?;
        tokio::spawn(
            self.accept(endpoint, incoming, state, queue)
                .unwrap_or_else(|e| panic!("{}: {:?}", e, e.cause)),
        );
        Ok(())
    }
}
impl QuicListener {
    async fn accept(
        self: Arc<Self>,
        _endpoint: Endpoint,
        mut incoming: Incoming,
        state: Arc<GlobalState>,
        queue: Sender<ContextRef>,
    ) -> Result<(), Error> {
        while let Some(conn) = incoming.next().await {
            let source = conn.remote_address();
            let source = crate::common::try_map_v4_addr(source);
            debug!("{}: QUIC connected from {:?}", self.name, source);
            match conn.await.context("connection") {
                Ok(conn) => {
                    let this = self.clone();
                    let state = state.clone();
                    let queue = queue.clone();
                    tokio::spawn(this.client_thread(conn, source, state, queue));
                }
                Err(e) => {
                    warn!("{}, Accept error: {}: cause: {:?}", self.name, e, e.cause);
                }
            }
        }
        Ok(())
    }
    async fn client_thread(
        self: Arc<Self>,
        conn: NewConnection,
        source: SocketAddr,
        state: Arc<GlobalState>,
        queue: Sender<ContextRef>,
    ) {
        let sessions = Arc::new(CHashMap::new());
        let mut bi_streams = conn.bi_streams;
        tokio::spawn(quic_frames_thread(
            self.name.to_owned(),
            sessions.clone(),
            conn.datagrams,
        ));
        let conn = conn.connection;
        while let Some(stream) = bi_streams.next().await {
            let stream = match stream {
                Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                    info!("{}: QUIC connection closed", self.name);
                    break;
                }
                Err(e) => {
                    warn!("{}: QUIC connection error: {}", self.name, e);
                    break;
                }
                Ok(s) => s,
            };
            debug!("{}: BiStream connected from {:?}", self.name, source);
            let stream: QuicStream = stream.into();
            let stream = make_buffered_stream(stream);
            let ctx = state
                .contexts
                .create_context(self.name.to_owned(), source)
                .await;
            ctx.write().await.set_client_stream(stream);
            let this = self.clone();
            let conn = conn.clone();
            let sessions = sessions.clone();
            tokio::spawn(
                h11c_handshake(ctx, queue.clone(), |_ch, id| async move {
                    Ok(create_quic_frames(conn, id, sessions).await)
                })
                .unwrap_or_else(move |e| {
                    warn!("{}: h11c handshake error: {}: {:?}", this.name, e, e.cause)
                }),
            );
        }
    }
}

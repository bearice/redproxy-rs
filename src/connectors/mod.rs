use std::{collections::HashMap, sync::Arc};

use crate::{context::ContextRef, GlobalState};
use async_trait::async_trait;
use easy_error::{bail, err_msg, Error};
use serde::Serialize;
use serde_yaml::Value;

mod direct;
mod http;
mod loadbalance;
#[cfg(feature = "quic")]
mod quic;
mod socks;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Feature {
    TcpForward,
    TcpBind,
    UdpForward,
    UdpBind,
}

impl Default for Feature {
    fn default() -> Self {
        Feature::TcpForward
    }
}

#[async_trait]
pub trait Connector: Send + Sync {
    async fn init(&mut self) -> Result<(), Error> {
        Ok(())
    }
    async fn verify(&self, _state: Arc<GlobalState>) -> Result<(), Error> {
        Ok(())
    }
    async fn connect(
        self: Arc<Self>,
        state: Arc<GlobalState>,
        ctx: ContextRef,
    ) -> Result<(), Error>;
    fn name(&self) -> &str;
    fn features(&self) -> &[Feature] {
        &[Feature::TcpForward]
    }
    fn has_feature(&self, feature: Feature) -> bool {
        self.features().contains(&feature)
    }
}

pub type ConnectorRef = Box<dyn Connector>;
pub fn from_config(cfg: &[Value]) -> Result<HashMap<String, Arc<dyn Connector>>, Error> {
    let mut ret: HashMap<String, Arc<dyn Connector>> = Default::default();
    for val in cfg {
        let r = from_value(val)?;
        let old = ret.insert(r.name().to_owned(), r.into());
        if let Some(old) = old {
            bail!("duplicate connector name: {}", old.name());
        }
    }
    Ok(ret)
}

pub fn from_value(value: &Value) -> Result<ConnectorRef, Error> {
    let name = value
        .get("name")
        .ok_or_else(|| err_msg("missing connector name"))?;
    if name == "deny" {
        bail!("connector name \"deny\" is reserved")
    }
    let tname = value.get("type").unwrap_or(name).as_str().unwrap();
    match tname {
        "direct" => direct::from_value(value),
        "http" => http::from_value(value),
        "socks" => socks::from_value(value),
        "loadbalance" => loadbalance::from_value(value),
        #[cfg(feature = "quic")]
        "quic" => quic::from_value(value),

        name => bail!("unknown connector type: {:?}", name),
    }
}

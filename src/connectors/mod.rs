use crate::context::Context;
use async_trait::async_trait;
use easy_error::{err_msg, Error};
use serde_yaml::{Sequence, Value};

mod copy;
pub mod direct;
#[async_trait]
pub trait Connector: std::fmt::Debug {
    async fn init(&mut self) -> Result<(), Error>;
    async fn connect(&self, ctx: Context) -> Result<(), Error>;
    fn name(&self) -> &str;
}

pub fn config(connectors: &Sequence) -> Result<Vec<Box<dyn Connector>>, Error> {
    let mut ret = Vec::with_capacity(connectors.len());
    for c in connectors {
        let c = from_value(c)?;
        ret.push(c);
    }
    Ok(ret)
}

pub fn from_value(value: &Value) -> Result<Box<dyn Connector>, Error> {
    let name = value.get("name").ok_or(err_msg("missing name"))?;
    let tname = value.get("type").or(Some(name)).unwrap();
    match tname.as_str() {
        Some("direct") => direct::from_value(value),
        _ => Err(err_msg("not implemented")),
    }
}

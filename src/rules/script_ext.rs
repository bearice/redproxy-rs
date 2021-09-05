use cidr::AnyIpCidr;
use easy_error::{bail, Error};
use log::warn;
use milu::script::{Call, ScriptContext};
use milu::{
    function,
    script::{Accessible, Callable, Evaluatable, NativeObject, ScriptContextRef, Type, Value},
};
use std::sync::Arc;
use std::{convert::TryInto, net::IpAddr};

use crate::context::{ContextProps, TargetAddress};

pub fn create_context(props: Arc<ContextProps>) -> ScriptContext {
    let mut ctx = ScriptContext::new(Some(Default::default()));
    let adapter = ContextAdaptor::new(props);
    ctx.set("request".to_string(), adapter.into());
    ctx.set("cidr_match".to_string(), CidrMatch::stub().into());
    ctx
}

#[derive(Clone, Hash)]
struct ContextAdaptor {
    req: Arc<ContextProps>,
}

impl<'a> ContextAdaptor {
    fn new(req: Arc<ContextProps>) -> Self {
        Self { req }
    }
}

impl Accessible for ContextAdaptor {
    fn names(&self) -> Vec<&str> {
        vec!["listener", "source", "target"]
    }

    fn get(&self, name: &str) -> Result<Value, Error> {
        match name {
            "listener" => Ok(self.req.listener.clone().into()),
            "target" => Ok(self.req.target.clone().into()),
            "source" => Ok(self.req.source.to_string().into()),
            _ => bail!("property undefined: {}", name),
        }
    }

    fn type_of(&self, name: &str, ctx: ScriptContextRef) -> Result<Type, Error> {
        match name {
            "listener" | "source" => Ok(Type::String),
            "target" => self.get(name)?.type_of(ctx),
            _ => bail!("undefined"),
        }
    }
}

impl NativeObject for ContextAdaptor {
    fn as_accessible(&self) -> Option<&dyn Accessible> {
        Some(self)
    }
}

impl std::fmt::Display for ContextAdaptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ContextAdaptor(id={})", self.req.id)
    }
}

impl std::fmt::Debug for ContextAdaptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ContextAdaptor(id={})", self.req.id)
    }
}

impl NativeObject for TargetAddress {
    fn as_evaluatable(&self) -> Option<&dyn Evaluatable> {
        Some(self)
    }
    fn as_accessible(&self) -> Option<&dyn Accessible> {
        Some(self)
    }
}

impl Evaluatable for TargetAddress {
    fn type_of(&self, _ctx: ScriptContextRef) -> Result<Type, Error> {
        Ok(Type::String)
    }

    fn value_of(&self, _ctx: ScriptContextRef) -> Result<Value, Error> {
        Ok(self.to_string().into())
    }
}

impl Accessible for TargetAddress {
    fn names(&self) -> Vec<&str> {
        vec!["host", "port", "type"]
    }

    fn get(&self, name: &str) -> Result<Value, Error> {
        match name {
            "host" => Ok(self.host().into()),
            "port" => Ok(self.port().into()),
            "type" => Ok(self.r#type().into()),
            _ => bail!("property undefined: {}", name),
        }
    }

    fn type_of<'b>(&self, name: &str, _ctx: ScriptContextRef) -> Result<Type, Error> {
        match name {
            "host" | "port" | "type" => Ok(Type::String),
            _ => bail!("undefined"),
        }
    }
}

function!(CidrMatch(ip: String, cidr: String)=>Type::Boolean, self, {
    let s_ip:String = ip.try_into()?;
    let s_cidr:String = cidr.try_into()?;
    let ip = s_ip.parse();
    if ip.is_err() {
        warn!("can not parse ip: {}", s_ip);
        return Ok(false.into())
    }
    let cidr = s_cidr.parse();
    if cidr.is_err() {
        warn!("can not parse cidr: {}", s_cidr);
        return Ok(false.into())
    }
    let ip: IpAddr = ip.unwrap();
    let cidr: AnyIpCidr = cidr.unwrap();
    Ok(cidr.contains(&ip).into())
});

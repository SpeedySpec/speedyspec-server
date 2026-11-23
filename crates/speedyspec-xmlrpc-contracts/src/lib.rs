//! The XML-RPC transport's contracts.
//!
//! [`XmlRpcBinding`] says which method name serves an operation and how its positional parameters map to the input;
//! [`MethodCall`], [`MethodResponse`] and [`Fault`] are what the codec reads and writes; [`MethodEntry`] is one row of
//! the served method table. The ports are [`XmlRpcMapping`] and [`FaultMapping`] (Strategy), [`XmlRpcHooks`]
//! (WordPress's XML-RPC hooks) and [`XmlRpcLogin`] (credentials to a caller). Depends only on
//! `speedyspec-operation-contracts`. Implementations: `speedyspec-xmlrpc` and `speedyspec-xmlrpc-php`.

mod binding;
mod message;
mod ports;

pub use binding::{XmlRpcBinding, XmlRpcParam};
pub use message::{Fault, MethodCall, MethodEntry, MethodResponse};
pub use ports::{FaultMapping, NoXmlRpcHooks, XmlRpcHooks, XmlRpcLogin, XmlRpcMapping};

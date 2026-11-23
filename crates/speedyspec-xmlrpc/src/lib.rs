//! The XML-RPC transport.
//!
//! [`codec`] reads and writes XML-RPC documents (Adapter). [`DefaultXmlRpcMapping`] names operations' methods and
//! [`DefaultFaultMapping`] gives WordPress's fault codes (Strategy); [`XmlRpcTransport`] installs them as the `xmlrpc`
//! transport. Contracts live in `speedyspec-xmlrpc-contracts`.

pub mod codec;
mod faults;
mod mapping;
mod transport;

pub use faults::DefaultFaultMapping;
pub use mapping::DefaultXmlRpcMapping;
pub use transport::{XMLRPC, XmlRpcTransport};

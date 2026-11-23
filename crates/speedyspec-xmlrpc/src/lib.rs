//! The XML-RPC transport.
//!
//! [`codec`] reads and writes XML-RPC documents (Adapter). [`DefaultXmlRpcMapping`] names operations' methods and
//! [`DefaultFaultMapping`] gives WordPress's fault codes (Strategy); [`XmlRpcTransport`] installs them as the `xmlrpc`
//! transport; [`XmlRpcServer`] serves `xmlrpc.php` (Template Method) and [`XmlRpcMiddleware`] puts it in the HTTP
//! pipeline. Contracts live in `speedyspec-xmlrpc-contracts`.

pub mod codec;
mod faults;
mod mapping;
mod middleware;
mod server;
mod transport;

pub use faults::DefaultFaultMapping;
pub use mapping::DefaultXmlRpcMapping;
pub use middleware::{XmlRpcMiddleware, XmlRpcServerProvider};
pub use server::{DEFAULT_ELEMENT_LIMIT, XmlRpcServer, category_of, error_of};
pub use transport::{XMLRPC, XmlRpcTransport};

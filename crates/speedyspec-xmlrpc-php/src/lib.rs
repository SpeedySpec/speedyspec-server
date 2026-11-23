//! The XML-RPC transport's PHP adapter.
//!
//! - [`PhpXmlRpcHooks`] fires WordPress's XML-RPC hooks (`xmlrpc_enabled` and the options before it,
//!   `xmlrpc_element_limit`, `xmlrpc_call`, `xmlrpc_login_error`), failing closed when one throws.
//! - [`PhpXmlRpcLogin`] logs in with `wp_authenticate()` and makes the user current.
//! - [`convert`] types PHP values as IXR does (`IXR_Date`, `IXR_Base64`, objects and arrays as structs or lists).
//! - [`XmlRpcArea`] gives PHP `SpeedySpec\XmlRpc\XmlRpc::bind( $operation )->method( $name )->params( … )`.

mod area;
pub mod convert;
mod hooks;
mod legacy;
mod login;

pub use area::XmlRpcArea;
pub use hooks::PhpXmlRpcHooks;
pub use login::PhpXmlRpcLogin;

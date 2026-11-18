//! Salvo wiring: the only crate besides the binary that knows the HTTP framework.

pub mod forwarded;
pub mod handlers;
pub mod live;
pub mod path;
pub mod service;

pub use forwarded::{ForwardedHeaders, RequestOrigin, origin};
pub use handlers::{AppHandle, Fallback, StaticApp};
pub use live::LiveApps;
pub use path::{PathRejection, normalize};
pub use service::{ServeState, build_router};

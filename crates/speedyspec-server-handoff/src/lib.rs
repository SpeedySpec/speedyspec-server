//! The keeper–generation protocol: versioned JSON-line messages over a Unix socketpair, with listening sockets
//! passed as `SCM_RIGHTS` alongside the message that names them. See `docs/handoff.md`.

mod channel;
mod protocol;

pub use channel::{AsyncChannel, Channel};
pub use protocol::{GenerationRole, PROTOCOL, SocketInfo, SocketKind, ToGeneration, ToKeeper};

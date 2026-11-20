//! ReDB as a NoSQL Store. ReDB locks its file to one process, so one owner process per site opens it
//! (`RedbOwner`, started through `run_owner`) and serves a framed protocol on a private Unix socket.

mod owner;
mod protocol;
mod socket;
mod store;

pub use owner::{OwnerConfig, OwnerError, RedbOwner, StopHandle, run_owner};
pub use protocol::{
    MAX_FRAME, PROTOCOL_VERSION, Request, Response, WireBound, read_frame, write_frame,
};
pub use socket::{check_socket_path, owner_socket_for};
pub use store::RedbStore;

//! The firewall at the edges: a native request stage the runtime runs before PHP ([`FirewallStage`]), the
//! WordPress hook callbacks that feed it ([`register_hooks`]), and the composition root that wires it all once per
//! worker ([`FirewallComposition`]).

mod compose;
mod hooks;
mod stage;

pub use compose::{FirewallComposition, StderrLog};
pub use hooks::register_hooks;
pub use stage::{CurrentClient, FirewallStage};

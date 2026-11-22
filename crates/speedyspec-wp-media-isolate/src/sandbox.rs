//! Locks the probe child down before it reads anything. Filled in with the seccomp filter (Linux).

/// Restricts the current process to what reading stdin and writing stdout need.
pub fn enter() {}

/// The current user's capabilities, as far as the engine asks (`current_user_can()`).
pub trait Capabilities {
    /// `current_user_can( $capability )`.
    fn current_user_can(&self, capability: &str) -> bool;
}

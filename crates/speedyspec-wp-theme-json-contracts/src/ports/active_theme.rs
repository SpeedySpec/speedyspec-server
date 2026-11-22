/// The active theme, as far as the engine asks about it (the resolver's questions join in plan 4).
pub trait ActiveTheme {
    /// `current_theme_supports( $feature )`.
    fn supports(&self, feature: &str) -> bool;
}

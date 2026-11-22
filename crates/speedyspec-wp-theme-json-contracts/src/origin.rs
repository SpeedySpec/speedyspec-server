/// Where a theme.json layer comes from, in WordPress's `VALID_ORIGINS` order.
///
/// Presets are keyed by origin once a document is constructed, and stylesheets are generated for a chosen set of
/// origins, so the order is part of the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Origin {
    /// WordPress's own `theme.json`.
    Default,
    /// Styles registered by blocks.
    Blocks,
    /// The active theme (and its parent).
    Theme,
    /// The user's global styles.
    Custom,
}

impl Origin {
    /// Every origin, in WordPress's order.
    pub const ALL: [Origin; 4] = [Origin::Default, Origin::Blocks, Origin::Theme, Origin::Custom];

    /// The name WordPress uses for the origin.
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Default => "default",
            Origin::Blocks => "blocks",
            Origin::Theme => "theme",
            Origin::Custom => "custom",
        }
    }

    /// Reads an origin name exactly as WordPress spells it.
    pub fn parse(name: &[u8]) -> Option<Origin> {
        Origin::ALL.into_iter().find(|o| o.as_str().as_bytes() == name)
    }

    /// Reads an origin as `WP_Theme_JSON`'s constructor does: anything unknown is the theme.
    pub fn from_wp(name: &[u8]) -> Origin {
        Origin::parse(name).unwrap_or(Origin::Theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_wp_falls_back_to_theme() {
        assert_eq!(Origin::from_wp(b"bogus"), Origin::Theme);
        assert_eq!(Origin::from_wp(b"custom"), Origin::Custom);
    }

    #[test]
    fn all_is_wordpress_order() {
        assert_eq!(Origin::ALL.map(Origin::as_str), ["default", "blocks", "theme", "custom"]);
    }

    #[test]
    fn parse_rejects_unknown() {
        assert_eq!(Origin::parse(b"blocks"), Some(Origin::Blocks));
        assert_eq!(Origin::parse(b"Theme"), None);
    }
}

use std::rc::Rc;

use crate::Value;

/// One registered block type, as the block registries describe it.
///
/// These are the raw registry facts; the engine computes selectors from them, so a block registry ported to Rust
/// later only has to report the same facts.
#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredBlock {
    /// The block name, for example `core/group`.
    pub name: String,
    /// `block.json`'s `selectors` (`$block_type->selectors`).
    pub selectors: Value,
    /// `block.json`'s `supports` (`$block_type->supports`).
    pub supports: Value,
    /// The block's style variation names: `block.json`'s `styles`, then the block styles registry's, without
    /// repeats, in that order.
    pub style_names: Vec<String>,
}

/// The registered blocks and their style variations (WordPress's `WP_Block_Type_Registry` and
/// `WP_Block_Styles_Registry`).
pub trait BlockMetadata {
    /// Every registered block, in registration order.
    fn registered(&self) -> Rc<Vec<RegisteredBlock>>;
    /// A value that changes whenever a block or a block style is registered or unregistered.
    fn change_marker(&self) -> u64;
}

use speedyspec_wp_shortcode_contracts::{ContextGuard, PhpValue, ShortcodeError, ShortcodeFilters};

/// The Null Object filters: nothing listens, so every value passes through and nothing is built.
pub struct NoFilters;

impl ShortcodeFilters for NoFilters {
    fn pre_do_shortcode_tag(&self, _: &[u8], _: &PhpValue, _: &PhpValue) -> Result<PhpValue, ShortcodeError> {
        Ok(PhpValue::Bool(false))
    }

    fn do_shortcode_tag(&self, output: PhpValue, _: &[u8], _: &PhpValue, _: &PhpValue) -> Result<PhpValue, ShortcodeError> {
        Ok(output)
    }

    fn shortcode_atts(&self, _: &[u8], out: PhpValue, _: &PhpValue, _: &PhpValue) -> Result<PhpValue, ShortcodeError> {
        Ok(out)
    }

    fn strip_shortcodes_tagnames(&self, tags: PhpValue, _: &[u8]) -> Result<PhpValue, ShortcodeError> {
        Ok(tags)
    }

    fn enter_do_shortcode_context(&self) -> Result<ContextGuard, ShortcodeError> {
        Ok(ContextGuard::new(false))
    }

    fn leave_do_shortcode_context(&self, _: ContextGuard) -> Result<(), ShortcodeError> {
        Ok(())
    }
}

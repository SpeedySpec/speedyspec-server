use std::rc::Rc;

use speedyspec_wp_hooks::{Callback, CallbackId, Count, HasFilterResult, HookApi, HookError, HookName, HookValue};
use speedyspec_wp_shortcode_contracts::{ContextGuard, PhpValue, ShortcodeError, ShortcodeFilters, Thrown, php_bytes};

/// The filters through the Rust Hook API, so PHP plugins' filters keep working (an Adapter).
///
/// Every filter goes through [`HookApi::skip_if_empty`], as the options port does: when nothing listens no hook
/// values are built, yet `did_filter()` still counts and `all` callbacks still run.
pub struct HookApiFilters {
    hooks: Rc<HookApi>,
}

/// WordPress's callback name, so PHP's `has_filter()` and `remove_filter()` find the same callback.
const CONTEXT_CALLBACK: &str = "_filter_do_shortcode_context";
const CONTEXT_HOOK: &str = "wp_get_attachment_image_context";

fn error(e: HookError) -> ShortcodeError {
    match e.payload {
        Some(payload) => ShortcodeError::Thrown(Thrown { message: e.message, payload }),
        None => ShortcodeError::Filter(e.message),
    }
}

impl HookApiFilters {
    /// Filters over `hooks`.
    pub fn new(hooks: Rc<HookApi>) -> Self {
        Self { hooks }
    }

    fn filter(
        &self,
        hook: HookName,
        value: PhpValue,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<PhpValue, ShortcodeError> {
        if self.hooks.skip_if_empty(&hook, Count::Filters).map_err(error)? {
            return Ok(value);
        }
        self.hooks.apply_filters(&hook, value, args()).map_err(error)
    }
}

impl ShortcodeFilters for HookApiFilters {
    fn pre_do_shortcode_tag(&self, tag: &[u8], attr: &PhpValue, m: &PhpValue) -> Result<PhpValue, ShortcodeError> {
        self.filter(HookName::from("pre_do_shortcode_tag"), PhpValue::Bool(false), || {
            vec![php_bytes(tag.to_vec()), attr.clone(), m.clone()]
        })
    }

    fn do_shortcode_tag(
        &self,
        output: PhpValue,
        tag: &[u8],
        attr: &PhpValue,
        m: &PhpValue,
    ) -> Result<PhpValue, ShortcodeError> {
        self.filter(HookName::from("do_shortcode_tag"), output, || {
            vec![php_bytes(tag.to_vec()), attr.clone(), m.clone()]
        })
    }

    fn shortcode_atts(
        &self,
        shortcode: &[u8],
        out: PhpValue,
        pairs: &PhpValue,
        atts: &PhpValue,
    ) -> Result<PhpValue, ShortcodeError> {
        let name = HookName::new(php_bytes([b"shortcode_atts_".as_slice(), shortcode].concat()));
        self.filter(name, out, || vec![pairs.clone(), atts.clone(), php_bytes(shortcode.to_vec())])
    }

    fn strip_shortcodes_tagnames(&self, tags: PhpValue, content: &[u8]) -> Result<PhpValue, ShortcodeError> {
        self.filter(HookName::from("strip_shortcodes_tagnames"), tags, || vec![php_bytes(content.to_vec())])
    }

    fn enter_do_shortcode_context(&self) -> Result<ContextGuard, ShortcodeError> {
        let hook = HookName::from(CONTEXT_HOOK);
        let id = CallbackId(Some(CONTEXT_CALLBACK.into()));
        if self.hooks.has_filter(&hook, Some(&id), None).map_err(error)? != HasFilterResult::No {
            return Ok(ContextGuard::new(false));
        }
        let callback =
            Callback::rust(CONTEXT_CALLBACK, |_: &[HookValue]| Ok(HookValue::Str("do_shortcode".into())));
        let added = self.hooks.add_filter(&hook, callback, &HookValue::Int(10), 1).map_err(error)?;
        Ok(ContextGuard::new(added))
    }

    fn leave_do_shortcode_context(&self, guard: ContextGuard) -> Result<(), ShortcodeError> {
        if guard.added() {
            let hook = HookName::from(CONTEXT_HOOK);
            self.hooks
                .remove_filter(&hook, CallbackId(Some(CONTEXT_CALLBACK.into())), &HookValue::Int(10))
                .map_err(error)?;
        }
        Ok(())
    }
}

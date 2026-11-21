use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_wp_shortcode_contracts::{ShortcodeHandler, ShortcodeRegistry, ShortcodeTagNames, TagName, php_bytes};

use crate::handler::{PhpShortcodeHandler, RustHandlers};

const GLOBAL: &str = "shortcode_tags";

/// The Registry over the `$shortcode_tags` global itself, so PHP code that reads, swaps or restores the array
/// directly (`WP_Embed::run_shortcode()`, WordPress's tests) sees and changes the same registry.
///
/// Reads go straight to the array (keys and one entry, never a copy). Writes run in PHP, so a copy of the array
/// someone saved keeps its own entries.
pub struct PhpGlobalRegistry {
    rust: Rc<RustHandlers>,
}

impl PhpGlobalRegistry {
    pub(crate) fn new(rust: Rc<RustHandlers>) -> Self {
        Self { rust }
    }

    fn support(function: &str, args: &[Value]) {
        let name = Value::Str(format!("SpeedySpec\\WpShortcode\\Support\\{function}"));
        if let Err(err) = speedyspec_php::call(&name, args) {
            eprintln!("speedyspec_wordpress: writing $shortcode_tags failed: {err}");
        }
    }
}

impl ShortcodeTagNames for PhpGlobalRegistry {
    fn tag_names(&self) -> Vec<TagName> {
        speedyspec_php::global_array_keys(GLOBAL)
            .unwrap_or_default()
            .into_iter()
            .map(TagName::from_global)
            .collect()
    }

    /// A hash of the keys in order: it changes whenever the names or their order change, which is all a
    /// scanner's cache needs (scanners also compare the names themselves).
    fn generation(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        speedyspec_php::global_array_keys(GLOBAL).hash(&mut hasher);
        hasher.finish()
    }
}

impl ShortcodeRegistry for PhpGlobalRegistry {
    fn add(&self, tag: TagName, handler: Rc<dyn ShortcodeHandler>) {
        match handler.php_callable() {
            Some(callable) => Self::support("set_tag", &[php_bytes(tag.as_bytes().to_vec()), callable]),
            None => eprintln!(
                "speedyspec_wordpress: the shortcode {:?} has no PHP callable; register it as a native function",
                String::from_utf8_lossy(tag.as_bytes())
            ),
        }
    }

    fn remove(&self, tag: &[u8]) {
        Self::support("unset_tag", &[php_bytes(tag.to_vec())]);
    }

    fn remove_all(&self) {
        Self::support("clear_tags", &[]);
    }

    fn exists(&self, tag: &[u8]) -> bool {
        speedyspec_php::global_array_contains(GLOBAL, tag)
    }

    fn handler(&self, tag: &[u8]) -> Option<Rc<dyn ShortcodeHandler>> {
        let value = speedyspec_php::global_array_get(GLOBAL, tag)?;
        if let Value::Str(name) = &value
            && let Some(rust) = self.rust.find(name)
        {
            return Some(rust);
        }
        Some(Rc::new(PhpShortcodeHandler::new(value)))
    }
}

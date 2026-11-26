//! `OptionsApi`: the handler of every options command and query, over its ports.

use std::rc::Rc;

use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_hooks_contracts::{Count, HookName, HookValue, Key};
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCache};

use crate::{
    AutoloadPolicy, Deprecations, FixedProtectedOptions, HookSanitizing, Installing,
    InstallingFlag, LogDeprecations, Multisite, NoOptionGroups, OptionGroups, OptionRepository,
    OptionsError, ProtectedOptions, RustCodec, Sanitizing, SiteConfiguration, ValueCodec,
    WordPressAutoloadPolicy,
};

/// What the options API works through.
#[derive(Clone)]
pub struct OptionsDeps {
    pub hooks: Rc<HookApi>,
    /// The request's current object cache: `wp_cache_init()` and each request end select a new one.
    pub cache: Rc<CurrentObjectCache>,
    pub repository: Rc<dyn OptionRepository>,
    pub autoload: Rc<dyn AutoloadPolicy>,
    pub codec: Rc<dyn ValueCodec>,
    pub installing: Rc<dyn Installing>,
    pub multisite: Rc<dyn Multisite>,
    pub sanitizing: Rc<dyn Sanitizing>,
    pub deprecations: Rc<dyn Deprecations>,
    pub protected: Rc<dyn ProtectedOptions>,
    pub groups: Rc<dyn OptionGroups>,
}

impl OptionsDeps {
    /// Every Rust adapter: a single site, blog 1, not installing.
    pub fn rust(
        hooks: Rc<HookApi>,
        cache: Rc<CurrentObjectCache>,
        repository: Rc<dyn OptionRepository>,
    ) -> OptionsDeps {
        OptionsDeps {
            autoload: Rc::new(WordPressAutoloadPolicy::new(hooks.clone())),
            codec: Rc::new(RustCodec),
            installing: Rc::new(InstallingFlag::new()),
            multisite: Rc::new(SiteConfiguration {
                multisite: false,
                blog_id: 1,
                network_id: 1,
            }),
            sanitizing: Rc::new(HookSanitizing::new(hooks.clone())),
            deprecations: Rc::new(LogDeprecations),
            protected: Rc::new(FixedProtectedOptions),
            groups: Rc::new(NoOptionGroups),
            hooks,
            cache,
            repository,
        }
    }
}

/// WordPress's options API (`option.php`), one method per function.
pub struct OptionsApi {
    pub(crate) d: OptionsDeps,
}

impl OptionsApi {
    pub fn new(deps: OptionsDeps) -> OptionsApi {
        OptionsApi { d: deps }
    }

    pub fn deps(&self) -> &OptionsDeps {
        &self.d
    }

    /// `apply_filters()`, through the fast path when nothing listens. The result is as the last callback returned
    /// it (a PHP value may stay opaque); read it with [`HookValue::read`] to look inside.
    pub(crate) fn filter(
        &self,
        hook: &str,
        value: HookValue,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<HookValue, OptionsError> {
        let name = HookName::from(hook);
        if self.d.hooks.skip_if_empty(&name, Count::Filters)? {
            return Ok(value);
        }
        Ok(self.d.hooks.apply_filters(&name, value, args())?)
    }

    /// `do_action()`, through the fast path when nothing listens.
    pub(crate) fn action(
        &self,
        hook: &str,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<(), OptionsError> {
        let name = HookName::from(hook);
        if self.d.hooks.skip_if_empty(&name, Count::Actions)? {
            return Ok(());
        }
        Ok(self.d.hooks.do_action(&name, args())?)
    }

    /// The object cache current now.
    pub(crate) fn cache(&self) -> Rc<ObjectCache> {
        self.d.cache.current()
    }

    pub(crate) fn group() -> HookValue {
        HookValue::Str("options".into())
    }

    /// `! wp_installing() || ! is_multisite()`: whether `alloptions` goes through the cache.
    pub(crate) fn caches_alloptions(&self) -> bool {
        !self.d.installing.installing() || !self.d.multisite.is_multisite()
    }
}

/// A name as PHP's array key: a canonical decimal integer becomes an integer key.
pub(crate) fn key_of(name: &str) -> Key {
    let digits = name.strip_prefix('-').unwrap_or(name);
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !(digits.len() > 1 && digits.starts_with('0'))
        && name != "-0";
    match canonical.then(|| name.parse::<i64>().ok()).flatten() {
        Some(i) => Key::Int(i),
        None => Key::Str(name.to_string()),
    }
}

/// `isset($array[$name])`: present and not `null`. A non-array has no entries.
pub(crate) fn isset(array: &HookValue, name: &str) -> Option<HookValue> {
    let HookValue::Array(items) = array else {
        return None;
    };
    let key = key_of(name);
    items
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.clone())
        .filter(|v| !matches!(v, HookValue::Null))
}

/// `$array[$name] = $value` on a PHP array (a non-array is replaced, as the code paths here first make it one).
pub(crate) fn set_key(array: &mut HookValue, name: &str, value: HookValue) {
    if !matches!(array, HookValue::Array(_)) {
        *array = HookValue::Array(Vec::new());
    }
    let HookValue::Array(items) = array else {
        unreachable!()
    };
    let key = key_of(name);
    match items.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => items.push((key, value)),
    }
}

/// `unset($array[$name])`.
pub(crate) fn unset_key(array: &mut HookValue, name: &str) {
    if let HookValue::Array(items) = array {
        let key = key_of(name);
        items.retain(|(k, _)| *k != key);
    }
}

/// PHP's truthiness, as `if ( ! $value )` reads it.
pub(crate) fn truthy(value: &HookValue) -> bool {
    !crate::php_empty(value)
}

//! The gettext family as WordPress composes it (`translate()`, `translate_with_gettext_context()`, `_n()`, `_nx()`):
//! a lookup through the controller, then the two filters, each skipped when nothing is hooked to it (Facade over
//! the controller and the Hook API).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use speedyspec_wp_hooks::{Count, HookApi, HookError, HookName, HookStore, HookValue};

use crate::controller::TranslationController;
use crate::plural::PluralError;

/// Why a translation call failed: a hook callback threw, or the plural rule did.
#[derive(Debug)]
pub enum I18nError {
    Hook(HookError),
    Plural(PluralError),
}

impl fmt::Display for I18nError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            I18nError::Hook(e) => write!(f, "{e}"),
            I18nError::Plural(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for I18nError {}

impl From<HookError> for I18nError {
    fn from(e: HookError) -> Self {
        I18nError::Hook(e)
    }
}

/// What a translation call produced, lent to the caller so a translation can be copied straight out of the file.
pub enum Translated<'a> {
    /// The translation, unfiltered (nothing was hooked).
    Bytes(&'a [u8]),
    /// The caller's own argument `i`, unchanged: the original (`0`), or for a plural the singular (`0`) or plural
    /// (`1`) that English rules picked.
    Arg(usize),
    /// What the filters returned.
    Value(HookValue),
}

/// The per-domain filter names (`gettext_{$domain}` and the rest), built once per domain per request.
pub struct DomainFilterNames {
    pub gettext: HookName,
    pub gettext_with_context: HookName,
    pub ngettext: HookName,
    pub ngettext_with_context: HookName,
}

fn name(n: &str) -> HookName {
    HookName::new(HookValue::Str(n.to_owned()))
}

/// A byte string as a PHP-shaped value.
pub fn bytes_value(b: &[u8]) -> HookValue {
    match std::str::from_utf8(b) {
        Ok(s) => HookValue::Str(s.to_owned()),
        Err(_) => HookValue::Bytes(b.to_vec()),
    }
}

/// WordPress's translation functions over the controller and the Hook API.
pub struct I18nApi {
    hooks: Rc<HookApi>,
    controller: Rc<RefCell<TranslationController>>,
    gettext: HookName,
    gettext_with_context: HookName,
    ngettext: HookName,
    ngettext_with_context: HookName,
    all: HookName,
    /// The hook store resolved once for this request, when the host resolves stores per call.
    request_store: RefCell<Option<Rc<dyn HookStore>>>,
    names: RefCell<HashMap<String, Rc<DomainFilterNames>, foldhash::fast::RandomState>>,
}

impl I18nApi {
    pub fn new(hooks: Rc<HookApi>, controller: Rc<RefCell<TranslationController>>) -> I18nApi {
        I18nApi {
            hooks,
            controller,
            gettext: name("gettext"),
            gettext_with_context: name("gettext_with_context"),
            ngettext: name("ngettext"),
            ngettext_with_context: name("ngettext_with_context"),
            all: name("all"),
            request_store: RefCell::new(None),
            names: RefCell::new(HashMap::default()),
        }
    }

    pub fn controller(&self) -> &Rc<RefCell<TranslationController>> {
        &self.controller
    }

    pub fn hooks(&self) -> &Rc<HookApi> {
        &self.hooks
    }

    /// Ends the request: the controller forgets what was loaded, and the per-domain names are dropped.
    pub fn reset(&self) {
        self.controller.borrow_mut().reset();
        self.names.borrow_mut().clear();
    }

    /// The filter names for `domain`.
    pub fn domain_filter_names(&self, domain: &str) -> Rc<DomainFilterNames> {
        if let Some(n) = self.names.borrow().get(domain) {
            return n.clone();
        }
        let n = Rc::new(DomainFilterNames {
            gettext: name(&format!("gettext_{domain}")),
            gettext_with_context: name(&format!("gettext_with_context_{domain}")),
            ngettext: name(&format!("ngettext_{domain}")),
            ngettext_with_context: name(&format!("ngettext_with_context_{domain}")),
        });
        self.names.borrow_mut().insert(domain.to_owned(), n.clone());
        n
    }

    /// The hooks a `translate()` (or with `context`, `translate_with_gettext_context()`) call may fire, so a native
    /// path can see whether PHP would have to run.
    pub fn hooks_for_translate(&self, domain: &str, context: bool) -> [HookName; 2] {
        if context {
            [
                name("gettext_with_context"),
                name(&format!("gettext_with_context_{domain}")),
            ]
        } else {
            [name("gettext"), name(&format!("gettext_{domain}"))]
        }
    }

    /// The hooks an `_n()` (or with `context`, `_nx()`) call may fire.
    pub fn hooks_for_plural(&self, domain: &str, context: bool) -> [HookName; 2] {
        if context {
            [
                name("ngettext_with_context"),
                name(&format!("ngettext_with_context_{domain}")),
            ]
        } else {
            [name("ngettext"), name(&format!("ngettext_{domain}"))]
        }
    }

    /// `apply_filters( $hook, $value, ...$args )`, skipping (and counting) a filter with nothing hooked.
    pub fn filter(
        &self,
        hook: &str,
        value: HookValue,
        args: Vec<HookValue>,
    ) -> Result<HookValue, I18nError> {
        let name = HookName::new(HookValue::Str(hook.to_owned()));
        if self.hooks.skip_if_empty(&name, Count::Filters)? {
            return Ok(value);
        }
        Ok(self.hooks.apply_filters(&name, value, args)?)
    }

    /// `do_action( $hook, ...$args )`, skipping (and counting) an action with nothing hooked.
    pub fn action(&self, hook: &str, args: Vec<HookValue>) -> Result<(), I18nError> {
        let name = HookName::new(HookValue::Str(hook.to_owned()));
        if self.hooks.skip_if_empty(&name, Count::Actions)? {
            return Ok(());
        }
        Ok(self.hooks.do_action(&name, args)?)
    }

    /// Uses `store` for the checks and counts of [`I18nApi::filters_empty`] and the `*_if_unhooked` calls until
    /// set again (a host whose Hook API resolves its store on every access sets it once per request).
    pub fn set_request_store(&self, store: Option<Rc<dyn HookStore>>) {
        *self.request_store.borrow_mut() = store;
    }

    /// Whether a request store is set (for hosts checking their wiring).
    pub fn has_request_store(&self) -> bool {
        self.request_store.borrow().is_some()
    }

    fn store(&self) -> Rc<dyn HookStore> {
        match &*self.request_store.borrow() {
            Some(s) => s.clone(),
            None => self.hooks.store().clone(),
        }
    }

    /// Whether `names` have no callback (nor an `all` hook) in `store`.
    fn unhooked(&self, store: &dyn HookStore, first: &HookName, second: &HookName) -> bool {
        let has = |n: &HookName| n.key().map(|k| store.has_hook(k)).unwrap_or(true);
        !has(&self.all) && !has(first) && !has(second)
    }

    /// Counts both filters as run, as `skip_if_empty` does for a filter with nothing hooked.
    fn count(store: &dyn HookStore, first: &HookName, second: &HookName) {
        for n in [first, second] {
            if let Ok(k) = n.key() {
                store.increment_did(Count::Filters, k);
            }
        }
    }

    /// Whether neither filter of a call would run a callback (nor an `all` hook), without counting anything. A
    /// native function serves the call itself only then; otherwise it leaves the call to WordPress's PHP.
    pub fn filters_empty(&self, domain: &str, context: bool, plural: bool) -> bool {
        let (first, names) = self.pair(domain, context, plural);
        let second = Self::second(&names, context, plural);
        self.unhooked(&*self.store(), first, second)
    }

    /// For a translation found elsewhere (a plugin's `Translations` object): when neither filter of the call has a
    /// callback, counts both as WordPress counts them and answers true; false (nothing counted) otherwise.
    pub fn count_if_unhooked(&self, domain: &str, context: bool, plural: bool) -> bool {
        let (first, names) = self.pair(domain, context, plural);
        let second = Self::second(&names, context, plural);
        let store = self.store();
        if !self.unhooked(&*store, first, second) {
            return false;
        }
        Self::count(&*store, first, second);
        true
    }

    /// `translate()` or `translate_with_gettext_context()` when nothing is hooked to its filters: served and counted
    /// as WordPress would; `None` (nothing counted) when a filter has a callback.
    pub fn translate_if_unhooked<R>(
        &self,
        text: &[u8],
        context: Option<&[u8]>,
        domain: &str,
        out: impl FnOnce(Translated<'_>) -> R,
    ) -> Option<R> {
        let with_context = context.is_some();
        let context = context.unwrap_or(b"");
        let (first, names) = self.pair(domain, with_context, false);
        let second = Self::second(&names, with_context, false);
        let store = self.store();
        if !self.unhooked(&*store, first, second) {
            return None;
        }
        let mut controller = self.controller.borrow_mut();
        let found = controller.translate(text, context, domain, None);
        Self::count(&*store, first, second);
        Some(match found {
            Some(t) => out(Translated::Bytes(t)),
            None => out(Translated::Arg(0)),
        })
    }

    /// `_n()` or `_nx()` when nothing is hooked to its filters; `None` (nothing counted) when a filter has a
    /// callback or the plural rule throws.
    pub fn translate_plural_if_unhooked<R>(
        &self,
        single: &[u8],
        plural: &[u8],
        number: i64,
        context: Option<&[u8]>,
        domain: &str,
        out: impl FnOnce(Translated<'_>) -> R,
    ) -> Option<R> {
        let with_context = context.is_some();
        let context = context.unwrap_or(b"");
        let (first, names) = self.pair(domain, with_context, true);
        let second = Self::second(&names, with_context, true);
        let store = self.store();
        if !self.unhooked(&*store, first, second) {
            return None;
        }
        let mut controller = self.controller.borrow_mut();
        let found = controller
            .translate_plural(single, plural, number, context, domain, None)
            .ok()?;
        Self::count(&*store, first, second);
        Some(match found {
            Some(t) => out(Translated::Bytes(t)),
            None => out(Translated::Arg(usize::from(number != 1))),
        })
    }

    fn pair(
        &self,
        domain: &str,
        context: bool,
        plural: bool,
    ) -> (&HookName, Rc<DomainFilterNames>) {
        let names = self.domain_filter_names(domain);
        let first = match (plural, context) {
            (false, false) => &self.gettext,
            (false, true) => &self.gettext_with_context,
            (true, false) => &self.ngettext,
            (true, true) => &self.ngettext_with_context,
        };
        (first, names)
    }

    fn second(names: &DomainFilterNames, context: bool, plural: bool) -> &HookName {
        match (plural, context) {
            (false, false) => &names.gettext,
            (false, true) => &names.gettext_with_context,
            (true, false) => &names.ngettext,
            (true, true) => &names.ngettext_with_context,
        }
    }

    /// Whether the first filter has nothing hooked (counting it, as WordPress counts a run), and if so whether the
    /// second has nothing either. `(true, true)`: the caller hands back what it had.
    fn skips(&self, first: &HookName, second: &HookName) -> Result<(bool, bool), I18nError> {
        let skip_first = self.hooks.skip_if_empty(first, Count::Filters)?;
        let skip_second = skip_first && self.hooks.skip_if_empty(second, Count::Filters)?;
        Ok((skip_first, skip_second))
    }

    /// Runs what [`I18nApi::skips`] did not skip. Nothing may be borrowed from the controller here: a callback can
    /// translate again.
    fn run(
        &self,
        first: &HookName,
        second: &HookName,
        skip_first: bool,
        mut value: HookValue,
        args: impl Fn() -> Vec<HookValue>,
    ) -> Result<HookValue, I18nError> {
        if !skip_first {
            value = self.hooks.apply_filters(first, value, args())?;
            if self.hooks.skip_if_empty(second, Count::Filters)? {
                return Ok(value);
            }
        }
        Ok(self.hooks.apply_filters(second, value, args())?)
    }

    /// Both filters on a value that is already owned.
    fn filters(
        &self,
        domain: &str,
        context: bool,
        plural: bool,
        translation: HookValue,
        args: impl Fn() -> Vec<HookValue>,
    ) -> Result<HookValue, I18nError> {
        let (first, names) = self.pair(domain, context, plural);
        let second = Self::second(&names, context, plural);
        let (skip_first, skip_second) = self.skips(first, second)?;
        if skip_second {
            return Ok(translation);
        }
        self.run(first, second, skip_first, translation, args)
    }

    /// `translate()` (`context` empty) or `translate_with_gettext_context()` against the controller.
    pub fn translate<R>(
        &self,
        text: &[u8],
        context: Option<&[u8]>,
        domain: &str,
        out: impl FnOnce(Translated<'_>) -> R,
    ) -> Result<R, I18nError> {
        let with_context = context.is_some();
        let context = context.unwrap_or(b"");
        let (first, names) = self.pair(domain, with_context, false);
        let second = Self::second(&names, with_context, false);
        let args = || {
            let mut a = vec![bytes_value(text)];
            if with_context {
                a.push(bytes_value(context));
            }
            a.push(HookValue::Str(domain.to_owned()));
            a
        };
        let mut controller = self.controller.borrow_mut();
        let found = controller.translate(text, context, domain, None);
        let (value, skip_first) = match found {
            Some(t) => {
                let (skip_first, skip_second) = self.skips(first, second)?;
                if skip_second {
                    return Ok(out(Translated::Bytes(t)));
                }
                let value = bytes_value(t);
                drop(controller);
                (value, skip_first)
            }
            None => {
                drop(controller);
                let (skip_first, skip_second) = self.skips(first, second)?;
                if skip_second {
                    return Ok(out(Translated::Arg(0)));
                }
                (bytes_value(text), skip_first)
            }
        };
        let value = self.run(first, second, skip_first, value, args)?;
        Ok(out(Translated::Value(value)))
    }

    /// `_n()` (`context` empty) or `_nx()` against the controller: the form the file's rule picks, else English
    /// rules on the originals.
    pub fn translate_plural<R>(
        &self,
        single: &[u8],
        plural: &[u8],
        number: i64,
        context: Option<&[u8]>,
        domain: &str,
        out: impl FnOnce(Translated<'_>) -> R,
    ) -> Result<R, I18nError> {
        let with_context = context.is_some();
        let context = context.unwrap_or(b"");
        let (first, names) = self.pair(domain, with_context, true);
        let second = Self::second(&names, with_context, true);
        let args = || {
            let mut a = vec![
                bytes_value(single),
                bytes_value(plural),
                HookValue::Int(number),
            ];
            if with_context {
                a.push(bytes_value(context));
            }
            a.push(HookValue::Str(domain.to_owned()));
            a
        };
        let mut controller = self.controller.borrow_mut();
        let found = controller
            .translate_plural(single, plural, number, context, domain, None)
            .map_err(I18nError::Plural)?;
        let (value, skip_first) = match found {
            Some(t) => {
                let (skip_first, skip_second) = self.skips(first, second)?;
                if skip_second {
                    return Ok(out(Translated::Bytes(t)));
                }
                let value = bytes_value(t);
                drop(controller);
                (value, skip_first)
            }
            None => {
                drop(controller);
                let pick = usize::from(number != 1);
                let (skip_first, skip_second) = self.skips(first, second)?;
                if skip_second {
                    return Ok(out(Translated::Arg(pick)));
                }
                (
                    bytes_value(if pick == 0 { single } else { plural }),
                    skip_first,
                )
            }
        };
        let value = self.run(first, second, skip_first, value, args)?;
        Ok(out(Translated::Value(value)))
    }

    /// The two `gettext` (or `gettext_with_context`) filters on a translation that did not come from the controller
    /// (a plugin's own `Translations` object in `$l10n`): `args` are what follows the translation.
    pub fn filter_translation(
        &self,
        translation: HookValue,
        args: Vec<HookValue>,
        domain: &str,
        context: bool,
    ) -> Result<HookValue, I18nError> {
        self.filters(domain, context, false, translation, || args.clone())
    }

    /// The two `ngettext` (or `ngettext_with_context`) filters on a translation that did not come from the
    /// controller.
    pub fn filter_plural(
        &self,
        translation: HookValue,
        args: Vec<HookValue>,
        domain: &str,
        context: bool,
    ) -> Result<HookValue, I18nError> {
        self.filters(domain, context, true, translation, || args.clone())
    }
}

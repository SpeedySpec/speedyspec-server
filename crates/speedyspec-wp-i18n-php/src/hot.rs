//! The native `translate()`, `translate_with_gettext_context()`, `__()`, `_x()`, `_n()` and `_nx()`.
//!
//! A call is served here when its arguments are strings (and an integer count), `$l10n[$domain]` is exactly a
//! `WP_Translations` over the request's controller for that domain, and nothing is hooked to the call's two
//! filters; the filters are then counted as WordPress counts them. Any other call goes to WordPress's own function
//! (the facade's `SpeedySpec\WpI18n\Legacy\`), with the same arguments.

use speedyspec_php::ext_php_rs::args::Arg;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::ext_php_rs::ffi::{zend_call_known_function, zval};

use speedyspec_php::ext_php_rs::flags::DataType;
use speedyspec_php::ext_php_rs::types::{ZendStr, Zval};
use speedyspec_php::ext_php_rs::zend::ExecuteData;
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_i18n::Translated;

use crate::state::shared;
use crate::zv;

#[derive(Clone, Copy)]
struct Spec {
    /// Where each argument is: (context, domain), the plural ones also have single/plural/number at 0, 1, 2.
    plural: bool,
    context: Option<usize>,
    domain: usize,
    legacy: &'static str,
}

const TRANSLATE: Spec = Spec {
    plural: false,
    context: None,
    domain: 1,
    legacy: "speedyspec\\wpi18n\\legacy\\translate",
};
const UNDERSCORES: Spec = Spec {
    plural: false,
    context: None,
    domain: 1,
    legacy: "speedyspec\\wpi18n\\legacy\\__",
};
const WITH_CONTEXT: Spec = Spec {
    plural: false,
    context: Some(1),
    domain: 2,
    legacy: "speedyspec\\wpi18n\\legacy\\translate_with_gettext_context",
};
const X: Spec = Spec {
    plural: false,
    context: Some(1),
    domain: 2,
    legacy: "speedyspec\\wpi18n\\legacy\\_x",
};
const N: Spec = Spec {
    plural: true,
    context: None,
    domain: 3,
    legacy: "speedyspec\\wpi18n\\legacy\\_n",
};
const NX: Spec = Spec {
    plural: true,
    context: Some(3),
    domain: 4,
    legacy: "speedyspec\\wpi18n\\legacy\\_nx",
};

fn write(ret: &mut Zval, ex: &ExecuteData, t: Translated<'_>) {
    match t {
        Translated::Bytes(b) => ret.set_zend_string(ZendStr::new(b, false)),
        Translated::Arg(i) => zv::return_arg(ret, ex, i),
        // Only reachable with a callback on a filter, which this path never serves; written for completeness.
        Translated::Value(HookValue::Str(s)) => {
            ret.set_zend_string(ZendStr::new(s.as_bytes(), false))
        }
        Translated::Value(HookValue::Bytes(b)) => ret.set_zend_string(ZendStr::new(&b, false)),
        Translated::Value(_) => ret.set_null(),
    }
}

thread_local! {
    /// The translation handed to the facade's filter helper (`take_translation()`).
    static PENDING: std::cell::RefCell<Option<Zval>> = const { std::cell::RefCell::new(None) };
}

/// `take_translation()`: the translation the native function found, for the filter helper it is calling.
extern "C" fn native_take_translation(_ex: &mut ExecuteData, ret: &mut Zval) {
    if let Some(value) = PENDING.with(|p| p.borrow_mut().take()) {
        zv::return_value(ret, value);
    }
}

/// Serves the call when it can; false leaves it to WordPress's function.
fn fast(ex: &ExecuteData, ret: &mut Zval, spec: Spec) -> bool {
    let Some(state) = shared() else { return false };
    let leading = if spec.plural { 3 } else { 1 } + usize::from(spec.context.is_some());
    if zv::num_args(ex) < leading {
        return false;
    }
    // The domain must be a string to find its entry in $l10n, as WordPress's own lookup would.
    let default_domain;
    let domain_ptr = if zv::num_args(ex) > spec.domain {
        match zv::string_arg(ex, spec.domain) {
            Some(d) => d,
            None => return false,
        }
    } else {
        default_domain = zv::string_zval(b"default");
        match default_domain.zend_str() {
            Some(s) => s as *const _ as *mut _,
            None => return false,
        }
    };
    let Ok(domain) = std::str::from_utf8(zv::bytes(domain_ptr)) else {
        return false;
    };
    let Some(obj) = zv::l10n_entry(domain_ptr) else {
        return false;
    };
    let (handle, ce) = zv::object_parts(obj);
    let generation = state.generation.get();
    let wp_translations = zv::class(generation, "wp_translations");
    let ours = !wp_translations.is_null()
        && ce == wp_translations
        && matches!(state.translations.borrow().get(&handle), Some((d, 0)) if d == domain);
    let with_context = spec.context.is_some();

    // The common case: the request controller's own domain, string arguments, nothing hooked.
    if ours {
        let strings = zv::string_arg(ex, 0).zip(if spec.plural {
            zv::string_arg(ex, 1).zip(zv::long_arg(ex, 2)).map(Some)
        } else {
            Some(None)
        });
        let context: Option<&[u8]> = match spec.context {
            Some(i) => zv::string_arg(ex, i).map(|c| zv::bytes(c)),
            None => Some(b""),
        };
        if let (Some((text, plural)), Some(context)) = (strings, context) {
            let text = zv::bytes(text);
            let served = match plural {
                None => state.api.translate_if_unhooked(
                    text,
                    spec.context.map(|_| context),
                    domain,
                    |t| write(ret, ex, t),
                ),
                Some((p, n)) => state.api.translate_plural_if_unhooked(
                    text,
                    zv::bytes(p),
                    n,
                    spec.context.map(|_| context),
                    domain,
                    |t| write(ret, ex, t),
                ),
            };
            if served.is_some() {
                return true;
            }
            // Hooked: look the translation up here, and leave only the filters to PHP.
            let found: Option<Option<Zval>> = {
                let mut controller = state.api.controller().borrow_mut();
                match plural {
                    None => Some(
                        controller
                            .translate(text, context, domain, None)
                            .map(zv::string_zval),
                    ),
                    Some((p, n)) => controller
                        .translate_plural(text, zv::bytes(p), n, context, domain, None)
                        .ok()
                        .map(|t| t.map(zv::string_zval)),
                }
            };
            // A plural rule that throws is left to WordPress's function.
            let Some(found) = found else { return false };
            let translation = match found {
                Some(t) => t,
                None => {
                    let pick = if plural.is_some_and(|(_, n)| n != 1) {
                        1
                    } else {
                        0
                    };
                    match zv::arg_copy(ex, pick) {
                        Some(z) => z,
                        None => return false,
                    }
                }
            };
            return filtered(ex, ret, spec, generation, translation);
        }
    }

    // Otherwise: what $l10n[$domain]->translate() / translate_plural() returns, as WordPress calls it, with this
    // call's leading arguments.
    let function = zv::method(
        obj,
        if spec.plural {
            "translate_plural"
        } else {
            "translate"
        },
    );
    if function.is_null() {
        return false;
    }
    let (args, _) = zv::frame_args(ex);
    let translation = zv::call(function, obj, args, leading);
    if zv::exception_pending() {
        // The method threw, as it would have in WordPress, before any filter ran.
        return true;
    }
    if state
        .api
        .count_if_unhooked(domain, with_context, spec.plural)
    {
        zv::return_value(ret, translation);
        return true;
    }
    filtered(ex, ret, spec, generation, translation)
}

/// Runs the call's two filters (callbacks are hooked) on `translation`: WordPress's two apply_filters() calls,
/// with the same arguments, in the facade's helper for this function, which takes the call's own arguments and the
/// translation through take_translation().
fn filtered(
    ex: &ExecuteData,
    ret: &mut Zval,
    spec: Spec,
    generation: u64,
    translation: Zval,
) -> bool {
    let with_context = spec.context.is_some();
    let helper = match (spec.plural, with_context) {
        (false, false) => "speedyspec\\wpi18n\\legacy\\filter_gettext",
        (false, true) => "speedyspec\\wpi18n\\legacy\\filter_gettext_with_context",
        (true, false) => "speedyspec\\wpi18n\\legacy\\filter_ngettext",
        (true, true) => "speedyspec\\wpi18n\\legacy\\filter_ngettext_with_context",
    };
    let filter = zv::function(generation, helper);
    if filter.is_null() {
        return false;
    }
    PENDING.with(|p| *p.borrow_mut() = Some(translation));
    let (args, n) = zv::frame_args(ex);
    let value = zv::call(filter, std::ptr::null_mut(), args, n);
    PENDING.with(|p| p.borrow_mut().take());
    zv::return_value(ret, value);
    true
}

/// Calls WordPress's own function with this call's arguments.
fn slow(ex: &ExecuteData, ret: &mut Zval, spec: Spec) {
    let generation = shared().map_or(0, |s| s.generation.get());
    let function = zv::function(generation, spec.legacy);
    if function.is_null() {
        return;
    }
    let n = zv::num_args(ex);
    let params = zv::arg(ex, 0).unwrap_or(std::ptr::null_mut());
    // SAFETY: a live function; `params` points at this frame's `n` contiguous argument slots, which the engine
    // copies into the callee's frame; `ret` is the engine's return slot.
    unsafe {
        zend_call_known_function(
            function,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            ret as *mut Zval as *mut zval,
            n as u32,
            params,
            std::ptr::null_mut(),
        );
    }
}

/// Clears what a request may have left behind (a bailout between handing over a translation and taking it).
pub(crate) fn start_request() {
    PENDING.with(|p| {
        if let Ok(mut p) = p.try_borrow_mut() {
            // Not dropped: it may point into the previous request's heap.
            std::mem::forget(p.take());
        }
    });
}

fn serve(ex: &mut ExecuteData, ret: &mut Zval, spec: Spec) {
    // A panic must not unwind into the engine: the call then goes to WordPress's own function.
    let served = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fast(ex, ret, spec)))
        .unwrap_or(false);
    if let Some(s) = shared() {
        let counter = if served { &s.fast } else { &s.slow };
        counter.set(counter.get() + 1);
    }
    if !served {
        slow(ex, ret, spec);
    }
}

extern "C" fn native_translate(ex: &mut ExecuteData, ret: &mut Zval) {
    serve(ex, ret, TRANSLATE)
}

extern "C" fn native_underscores(ex: &mut ExecuteData, ret: &mut Zval) {
    serve(ex, ret, UNDERSCORES)
}

extern "C" fn native_with_context(ex: &mut ExecuteData, ret: &mut Zval) {
    serve(ex, ret, WITH_CONTEXT)
}

extern "C" fn native_x(ex: &mut ExecuteData, ret: &mut Zval) {
    serve(ex, ret, X)
}

extern "C" fn native_n(ex: &mut ExecuteData, ret: &mut Zval) {
    serve(ex, ret, N)
}

extern "C" fn native_nx(ex: &mut ExecuteData, ret: &mut Zval) {
    serve(ex, ret, NX)
}

fn args(f: FunctionBuilder<'static>, required: &[&str]) -> FunctionBuilder<'static> {
    let mut f = f;
    for name in required {
        f = f.arg(Arg::new(*name, DataType::Mixed));
    }
    f.not_required()
        .arg(Arg::new("domain", DataType::Mixed).default("'default'"))
        .returns(DataType::Mixed, false, true)
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    vec![
        FunctionBuilder::new(
            "SpeedySpec\\WpI18n\\Native\\take_translation",
            native_take_translation,
        )
        .returns(DataType::Mixed, false, true),
        args(
            FunctionBuilder::new("translate", native_translate),
            &["text"],
        ),
        args(FunctionBuilder::new("__", native_underscores), &["text"]),
        args(
            FunctionBuilder::new("translate_with_gettext_context", native_with_context),
            &["text", "context"],
        ),
        args(FunctionBuilder::new("_x", native_x), &["text", "context"]),
        args(
            FunctionBuilder::new("_n", native_n),
            &["single", "plural", "number"],
        ),
        args(
            FunctionBuilder::new("_nx", native_nx),
            &["single", "plural", "number", "context"],
        ),
    ]
}

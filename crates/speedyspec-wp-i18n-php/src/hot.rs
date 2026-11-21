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

/// Serves the call when it can; false leaves it to WordPress's function.
fn fast(ex: &ExecuteData, ret: &mut Zval, spec: Spec) -> bool {
    let Some(state) = shared() else { return false };
    let Some(text) = zv::string_arg(ex, 0) else {
        return false;
    };
    let (plural, number) = if spec.plural {
        let (Some(p), Some(n)) = (zv::string_arg(ex, 1), zv::long_arg(ex, 2)) else {
            return false;
        };
        (Some(p), n)
    } else {
        (None, 0)
    };
    let context: &[u8] = match spec.context {
        Some(i) => match zv::string_arg(ex, i) {
            Some(c) => zv::bytes(c),
            None => return false,
        },
        None => b"",
    };
    let domain_string = if zv::num_args(ex) > spec.domain {
        match zv::string_arg(ex, spec.domain) {
            Some(d) => Some(d),
            None => return false,
        }
    } else {
        None
    };
    let domain: &str = match domain_string {
        Some(d) => match std::str::from_utf8(zv::bytes(d)) {
            Ok(s) => s,
            Err(_) => return false,
        },
        None => "default",
    };

    // $l10n[$domain] must be the request controller's WP_Translations for this very domain.
    let entry = match domain_string {
        Some(d) => zv::l10n_entry(d),
        None => {
            let key = ZendStr::new("default", false);
            zv::l10n_entry(key.as_ptr() as *mut _)
        }
    };
    let Some(obj) = entry else { return false };
    let (handle, ce) = zv::object_parts(obj);
    let generation = state.generation.get();
    let wp_translations = zv::class(generation, "wp_translations");
    if wp_translations.is_null() || ce != wp_translations {
        return false;
    }
    match state.translations.borrow().get(&handle) {
        Some((d, 0)) if d == domain => {}
        _ => return false,
    }
    let with_context = spec.context.is_some();
    if !state.api.filters_empty(domain, with_context, spec.plural) {
        return false;
    }

    let text = zv::bytes(text);
    let result = match plural {
        None => state
            .api
            .translate(text, context, domain, |t| write(ret, ex, t)),
        Some(p) => state
            .api
            .translate_plural(text, zv::bytes(p), number, context, domain, |t| {
                write(ret, ex, t)
            }),
    };
    // An error (a plural rule that throws) is left to WordPress's function, which throws it; nothing was counted.
    result.is_ok()
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

fn serve(ex: &mut ExecuteData, ret: &mut Zval, spec: Spec) {
    let served = fast(ex, ret, spec);
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

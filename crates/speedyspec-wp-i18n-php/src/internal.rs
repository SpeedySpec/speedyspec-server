//! The native functions the facade's classes call (`SpeedySpec\WpI18n\Native\`). The class methods have already
//! coerced every argument to the type WordPress's signature names.

use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_i18n::{
    FileRef, LOCALE_STRINGS, LocaleSwitcher, PhpKey, PluralError, TEXT_DIRECTION, Translated,
    TranslationController, bytes_value,
};

use crate::effects::{PhpScriptSources, PhpSwitchEffects, native_error, php_string, port_error};
use crate::state::{Shared, shared};

const NATIVE: &str = "SpeedySpec\\WpI18n\\Native\\";

type R = Result<Value, NativeError>;

fn state() -> Result<Rc<Shared>, NativeError> {
    shared().ok_or_else(|| NativeError::throw("Error", "the translation area is not installed"))
}

fn bytes(args: &[Value], i: usize) -> Result<&[u8], NativeError> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.as_bytes()),
        Some(Value::Bytes(b)) => Ok(b),
        other => Err(NativeError::throw(
            "TypeError",
            format!("argument #{} must be a string, {other:?} given", i + 1),
        )),
    }
}

fn text(args: &[Value], i: usize) -> Result<String, NativeError> {
    Ok(String::from_utf8_lossy(bytes(args, i)?).into_owned())
}

fn opt_text(args: &[Value], i: usize) -> Result<Option<String>, NativeError> {
    match args.get(i) {
        None | Some(Value::Null) => Ok(None),
        _ => text(args, i).map(Some),
    }
}

fn int(args: &[Value], i: usize) -> i64 {
    match args.get(i) {
        Some(Value::Int(n)) => *n,
        _ => 0,
    }
}

fn path(args: &[Value], i: usize) -> Result<std::path::PathBuf, NativeError> {
    Ok(std::path::PathBuf::from(
        std::ffi::OsStr::from_encoded_bytes_unchecked_safe(bytes(args, i)?),
    ))
}

/// `OsStr` from bytes: on Unix any bytes are a path.
trait FromBytes {
    fn from_encoded_bytes_unchecked_safe(b: &[u8]) -> &std::ffi::OsStr;
}

impl FromBytes for std::ffi::OsStr {
    fn from_encoded_bytes_unchecked_safe(b: &[u8]) -> &std::ffi::OsStr {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::OsStr::from_bytes(b)
        }
        #[cfg(not(unix))]
        {
            std::ffi::OsStr::new(std::str::from_utf8(b).unwrap_or(""))
        }
    }
}

fn controller(
    args: &[Value],
) -> Result<Rc<std::cell::RefCell<TranslationController>>, NativeError> {
    state()?
        .controller(int(args, 0))
        .ok_or_else(|| NativeError::throw("Error", "no such translation controller"))
}

fn plural_error(e: PluralError) -> NativeError {
    NativeError::throw(e.php_class(), e.to_string())
}

fn key(k: &[u8]) -> Key {
    Key::Str(String::from_utf8_lossy(k).into_owned())
}

fn php_key(k: PhpKey) -> Key {
    match k {
        PhpKey::Int(n) => Key::Int(n),
        PhpKey::Str(s) => key(&s),
    }
}

fn headers(list: &[(String, String)]) -> Value {
    Value::Array(
        list.iter()
            .map(|(k, v)| (Key::Str(k.clone()), Value::Str(v.clone())))
            .collect(),
    )
}

fn register_translations(a: &[Value]) -> R {
    let s = state()?;
    let Some(handle) = a.first().and_then(speedyspec_php::object_id) else {
        return Ok(Value::Null);
    };
    let domain = text(a, 1)?;
    let controller = match a.get(2) {
        Some(Value::Int(n)) => *n,
        _ => -1,
    };
    s.translations
        .borrow_mut()
        .insert(handle as u32, (domain, controller));
    Ok(Value::Null)
}

fn controller_main(_: &[Value]) -> R {
    // A new singleton (WordPress makes one per request; tests make fresh ones) starts empty.
    state()?.api.controller().borrow_mut().reset();
    Ok(Value::Int(0))
}

fn load_file(a: &[Value]) -> R {
    let c = controller(a)?;
    let file = path(a, 1)?;
    let domain = text(a, 2)?;
    let locale = opt_text(a, 3)?;
    state()?.prefetch(&file, true);
    Ok(Value::Bool(c.borrow_mut().load_file(
        &file,
        &domain,
        locale.as_deref(),
    )))
}

fn unload_file(a: &[Value]) -> R {
    let c = controller(a)?;
    let file = match a.get(1) {
        None | Some(Value::Null) => std::path::PathBuf::new(),
        _ => path(a, 1)?,
    };
    let domain = text(a, 2)?;
    let locale = opt_text(a, 3)?;
    Ok(Value::Bool(c.borrow_mut().unload_file(
        FileRef::Path(&file),
        &domain,
        locale.as_deref(),
    )))
}

fn translate(a: &[Value]) -> R {
    let c = controller(a)?;
    let (t, ctx, domain, locale) = (bytes(a, 1)?, bytes(a, 2)?, text(a, 3)?, opt_text(a, 4)?);
    Ok(c.borrow_mut()
        .translate(t, ctx, &domain, locale.as_deref())
        .map_or(Value::Bool(false), bytes_value))
}

fn translate_plural(a: &[Value]) -> R {
    let c = controller(a)?;
    let plurals: Vec<Vec<u8>> = match a.get(1) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|(_, v)| match v {
                Value::Str(s) => Some(s.as_bytes().to_vec()),
                Value::Bytes(b) => Some(b.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let refs: Vec<&[u8]> = plurals.iter().map(Vec::as_slice).collect();
    let (n, ctx, domain, locale) = (int(a, 2), bytes(a, 3)?, text(a, 4)?, opt_text(a, 5)?);
    let mut c = c.borrow_mut();
    let found = c
        .translate_plurals(&refs, n, ctx, &domain, locale.as_deref())
        .map_err(plural_error)?;
    Ok(found.map_or(Value::Bool(false), bytes_value))
}

fn get_entries(a: &[Value]) -> R {
    let c = controller(a)?;
    let domain = text(a, 1)?;
    let entries = c.borrow().get_entries(&domain);
    Ok(Value::Array(
        entries
            .into_iter()
            .map(|(k, v)| (php_key(k), bytes_value(&v)))
            .collect(),
    ))
}

/// `WP_Translation_File::create()`'s choice: the given type, else the text after the last `.`.
fn file_type_of(a: &[Value]) -> Result<Option<&'static str>, NativeError> {
    let chosen: Vec<u8> = match a.get(1) {
        None | Some(Value::Null) => {
            let f = bytes(a, 0)?;
            f.iter()
                .rposition(|b| *b == b'.')
                .map(|i| f[i + 1..].to_vec())
                .unwrap_or_default()
        }
        _ => bytes(a, 1)?.to_vec(),
    };
    Ok(match chosen.as_slice() {
        b"mo" => Some("mo"),
        b"php" => Some("php"),
        _ => None,
    })
}

fn file_open(a: &[Value]) -> R {
    let s = state()?;
    let file = path(a, 0)?;
    if file_type_of(a)?.is_none() {
        return Ok(Value::Bool(false));
    }
    s.prefetch(&file, false);
    let Some(handle) = s.api.controller().borrow().open(&file) else {
        return Ok(Value::Bool(false));
    };
    let mut files = s.files.borrow_mut();
    files.push(handle);
    Ok(Value::Int(files.len() as i64 - 1))
}

fn file_handle(a: &[Value]) -> Result<Rc<speedyspec_wp_i18n::FileHandle>, NativeError> {
    let s = state()?;
    let files = s.files.borrow();
    usize::try_from(int(a, 0))
        .ok()
        .and_then(|i| files.get(i).cloned())
        .ok_or_else(|| NativeError::throw("Error", "no such translation file"))
}

fn file_parse(a: &[Value]) -> R {
    let h = file_handle(a)?;
    let file = h.parse();
    let entries = Value::Array(
        file.entries()
            .map(|(k, v)| (php_key(speedyspec_wp_i18n::php_key(k)), bytes_value(v)))
            .collect(),
    );
    let error = h.error().map_or(Value::Null, |e| Value::Str(e.to_string()));
    Ok(Value::Array(vec![
        (Key::Int(0), headers(file.headers())),
        (Key::Int(1), entries),
        (Key::Int(2), error),
    ]))
}

fn file_plural_form(a: &[Value]) -> R {
    let h = file_handle(a)?;
    let form = h.parse().plural_form(int(a, 1)).map_err(plural_error)?;
    Ok(Value::Int(form))
}

/// `WP_Locale::init()`'s translations (and the text direction's when asked), served here only when WordPress's
/// `__()` and `_x()` would end in the request's controller with nothing hooked; `null` leaves them to PHP.
fn locale_strings(a: &[Value]) -> R {
    let s = state()?;
    let ask = matches!(a.first(), Some(Value::Bool(true)));
    if !crate::hot::entry_is_ours("default")
        || !s.api.filters_empty("default", false, false)
        || !s.api.filters_empty("default", true, false)
    {
        return Ok(Value::Null);
    }
    let direction = (TEXT_DIRECTION.0, Some(TEXT_DIRECTION.1));
    let lookups = LOCALE_STRINGS
        .iter()
        .copied()
        .chain(ask.then_some(direction));
    let mut out = Vec::with_capacity(LOCALE_STRINGS.len() + 1);
    for (i, (text, context)) in lookups.enumerate() {
        let value = s
            .api
            .translate_if_unhooked(
                text.as_bytes(),
                context.map(str::as_bytes),
                "default",
                |t| match t {
                    Translated::Bytes(b) => bytes_value(b),
                    Translated::Arg(_) => Value::Str(text.to_owned()),
                    Translated::Value(v) => v,
                },
            )
            .ok_or_else(|| {
                NativeError::throw(
                    "Error",
                    "a gettext filter was hooked while WP_Locale was built",
                )
            })?;
        out.push((Key::Int(i as i64), value));
    }
    Ok(Value::Array(out))
}

/// The switcher registered for the object in argument `i`.
fn switcher(a: &[Value]) -> Result<Rc<LocaleSwitcher>, NativeError> {
    let id = a
        .first()
        .and_then(speedyspec_php::object_id)
        .ok_or_else(|| NativeError::throw("Error", "not a locale switcher"))?;
    state()?
        .switchers
        .borrow()
        .get(&(id as u32))
        .cloned()
        .ok_or_else(|| NativeError::throw("Error", "this WP_Locale_Switcher was not constructed"))
}

fn effects() -> Result<PhpSwitchEffects, NativeError> {
    Ok(PhpSwitchEffects {
        api: state()?.api.clone(),
    })
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or(Value::Null)
}

fn or_false(v: Option<Value>) -> Value {
    v.unwrap_or(Value::Bool(false))
}

fn switcher_new(a: &[Value]) -> R {
    let id = a
        .first()
        .and_then(speedyspec_php::object_id)
        .ok_or_else(|| NativeError::throw("Error", "not a locale switcher"))?;
    let available = match a.get(2) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|(_, v)| match v {
                Value::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    state()?.switchers.borrow_mut().insert(
        id as u32,
        Rc::new(LocaleSwitcher::new(arg(a, 1), available)),
    );
    Ok(Value::Null)
}

/// The stack, oldest first, as WordPress keeps it: `[locale, user_id]` pairs.
fn switcher_stack(a: &[Value]) -> R {
    Ok(Value::Array(
        switcher(a)?
            .stack()
            .into_iter()
            .enumerate()
            .map(|(i, (locale, user))| {
                (
                    Key::Int(i as i64),
                    Value::Array(vec![(Key::Int(0), locale), (Key::Int(1), user)]),
                )
            })
            .collect(),
    ))
}

/// `load_script_translations( $file, $handle, $domain )`.
fn load_script_translations(a: &[Value]) -> R {
    let s = state()?;
    s.api
        .load_script_translations(
            &s.script_files,
            arg(a, 0),
            &php_string(&arg(a, 1)),
            &php_string(&arg(a, 2)),
        )
        .map_err(native_error)
}

/// `_load_script_textdomain_from_src( string $handle, string $src, string $domain, string $path, bool $is_module )`.
fn load_script_textdomain_from_src(a: &[Value]) -> R {
    let s = state()?;
    let is_module = speedyspec_wp_i18n_contracts::php_truthy(&arg(a, 4));
    s.api
        .load_script_textdomain_from_src(
            &PhpScriptSources,
            &s.script_files,
            &php_string(&arg(a, 0)),
            &php_string(&arg(a, 1)),
            &php_string(&arg(a, 2)),
            &php_string(&arg(a, 3)),
            is_module,
        )
        .map_err(native_error)
}

/// The natives declared under WordPress's own names.
pub(crate) fn wordpress_functions() -> Vec<FunctionBuilder<'static>> {
    vec![
        native::function("load_script_translations", load_script_translations),
        native::function(
            "_load_script_textdomain_from_src",
            load_script_textdomain_from_src,
        ),
    ]
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    let f =
        |name: &str, body: fn(&[Value]) -> R| native::function(&format!("{NATIVE}{name}"), body);
    vec![
        f("register_translations", register_translations),
        f("locale_strings", locale_strings),
        f("switcher_new", switcher_new),
        f("switcher_stack", switcher_stack),
        f("switcher_switch", |a| {
            let sw = switcher(a)?;
            sw.switch_to_locale(&effects()?, &arg(a, 1), &arg(a, 2))
                .map(Value::Bool)
                .map_err(port_error)
        }),
        f("switcher_switch_user", |a| {
            let sw = switcher(a)?;
            sw.switch_to_user_locale(&effects()?, &arg(a, 1))
                .map(Value::Bool)
                .map_err(port_error)
        }),
        f("switcher_restore", |a| {
            let sw = switcher(a)?;
            sw.restore_previous_locale(&effects()?)
                .map(or_false)
                .map_err(port_error)
        }),
        f("switcher_restore_current", |a| {
            let sw = switcher(a)?;
            sw.restore_current_locale(&effects()?)
                .map(or_false)
                .map_err(port_error)
        }),
        f("switcher_is_switched", |a| {
            Ok(Value::Bool(switcher(a)?.is_switched()))
        }),
        f("switcher_locale", |a| {
            Ok(or_false(switcher(a)?.switched_locale()))
        }),
        f("switcher_user_id", |a| {
            Ok(or_false(switcher(a)?.switched_user_id()))
        }),
        f("switcher_filter", |a| {
            Ok(switcher(a)?.filter_locale(arg(a, 1)))
        }),
        f("controller_new", |_| {
            Ok(Value::Int(state()?.new_controller()))
        }),
        f("controller_main", controller_main),
        f("controller_get_locale", |a| {
            Ok(Value::Str(controller(a)?.borrow().locale().to_owned()))
        }),
        f("controller_set_locale", |a| {
            controller(a)?.borrow_mut().set_locale(&text(a, 1)?);
            Ok(Value::Null)
        }),
        f("controller_load_file", load_file),
        f("controller_unload_file", unload_file),
        f("controller_unload_textdomain", |a| {
            let (domain, locale) = (text(a, 1)?, opt_text(a, 2)?);
            Ok(Value::Bool(
                controller(a)?
                    .borrow_mut()
                    .unload_textdomain(&domain, locale.as_deref()),
            ))
        }),
        f("controller_is_textdomain_loaded", |a| {
            let (domain, locale) = (text(a, 1)?, opt_text(a, 2)?);
            Ok(Value::Bool(
                controller(a)?
                    .borrow()
                    .is_textdomain_loaded(&domain, locale.as_deref()),
            ))
        }),
        f("controller_translate", translate),
        f("controller_translate_plural", translate_plural),
        f("controller_get_headers", |a| {
            Ok(headers(&controller(a)?.borrow().get_headers(&text(a, 1)?)))
        }),
        f("controller_get_entries", get_entries),
        f("controller_has_translation", |a| {
            let (singular, domain, locale) = (bytes(a, 1)?.to_vec(), text(a, 2)?, opt_text(a, 3)?);
            Ok(Value::Bool(controller(a)?.borrow_mut().has_translation(
                &singular,
                &domain,
                locale.as_deref(),
            )))
        }),
        f("file_open", file_open),
        f("file_type", |a| {
            Ok(Value::Str(file_type_of(a)?.unwrap_or("mo").to_owned()))
        }),
        f("file_parse", file_parse),
        f("file_plural_form", file_plural_form),
        f("hot_stats", |_| {
            let s = state()?;
            Ok(Value::Array(vec![
                (Key::Int(0), Value::Int(s.fast.get() as i64)),
                (Key::Int(1), Value::Int(s.slow.get() as i64)),
                (Key::Int(2), Value::Bool(s.api.has_request_store())),
            ]))
        }),
    ]
}

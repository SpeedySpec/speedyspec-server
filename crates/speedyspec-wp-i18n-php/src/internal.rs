//! The native functions the facade's classes call (`SpeedySpec\WpI18n\Native\`). The class methods have already
//! coerced every argument to the type WordPress's signature names.

use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_i18n::{FileRef, PhpKey, PluralError, TranslationController, bytes_value};

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

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    let f =
        |name: &str, body: fn(&[Value]) -> R| native::function(&format!("{NATIVE}{name}"), body);
    vec![
        f("register_translations", register_translations),
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
            ]))
        }),
    ]
}

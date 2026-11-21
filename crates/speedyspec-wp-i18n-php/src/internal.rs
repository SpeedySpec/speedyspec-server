//! The native functions the facade's classes call (`SpeedySpec\WpI18n\Native\`). The class methods have already
//! coerced every argument to the type WordPress's signature names.

use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_i18n::{
    FileRef, LOCALE_STRINGS, LocaleSwitcher, PhpKey, PluralError, PluralRule, PluralToken,
    TEXT_DIRECTION, Translated, TranslationController, bytes_value,
};

use speedyspec_wp_i18n_contracts::PhpArrayKey;
use speedyspec_wp_i18n_files::mo_pomo::MoPairs;
use speedyspec_wp_i18n_files::po;

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
    let kind = match file_type_of(a)? {
        Some("mo") => speedyspec_wp_i18n_contracts::FileKind::Mo,
        Some(_) => speedyspec_wp_i18n_contracts::FileKind::Php,
        None => return Ok(Value::Bool(false)),
    };
    s.prefetch_as(&file, false, kind);
    let Some(handle) = s.api.controller().borrow().open_as(&file, kind) else {
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

/// `Plural_Forms::parse( $str )`: WordPress's token list, or its exception.
fn plural_parse(a: &[Value]) -> R {
    let expression = text(a, 0)?;
    let rule = PluralRule::compile(&expression).map_err(plural_error)?;
    Ok(Value::Array(
        rule.tokens()
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let token = match t {
                    PluralToken::Var => vec![(Key::Int(0), Value::Str("var".into()))],
                    PluralToken::Value(v) => vec![
                        (Key::Int(0), Value::Str("value".into())),
                        (Key::Int(1), Value::Int(v)),
                    ],
                    PluralToken::Op(op) => vec![
                        (Key::Int(0), Value::Str("op".into())),
                        (Key::Int(1), Value::Str(op)),
                    ],
                };
                (Key::Int(i as i64), Value::Array(token))
            })
            .collect(),
    ))
}

/// `Plural_Forms::execute( $n )` for an integer `n` over the object's own tokens; `null` when the tokens are not
/// WordPress's shape (the facade then runs WordPress's PHP body).
fn plural_execute(a: &[Value]) -> R {
    let (Some(Value::Array(tokens)), Some(Value::Int(n))) =
        (a.first().map(|v| v.read().into_owned()), a.get(1))
    else {
        return Ok(Value::Null);
    };
    let mut program = Vec::with_capacity(tokens.len());
    for (_, token) in &tokens {
        let Value::Array(parts) = token else {
            return Ok(Value::Null);
        };
        let part = |i: i64| {
            parts
                .iter()
                .find(|(k, _)| *k == Key::Int(i))
                .map(|(_, v)| v)
        };
        program.push(match (part(0), part(1)) {
            (Some(Value::Str(k)), _) if k == "var" => PluralToken::Var,
            (Some(Value::Str(k)), Some(Value::Int(v))) if k == "value" => PluralToken::Value(*v),
            (Some(Value::Str(k)), Some(Value::Str(op))) if k == "op" => PluralToken::Op(op.clone()),
            _ => return Ok(Value::Null),
        });
    }
    PluralRule::from_tokens(&program)
        .eval(*n)
        .map(Value::Int)
        .map_err(plural_error)
}

fn count_pomo() -> Result<(), NativeError> {
    let s = state()?;
    s.pomo.set(s.pomo.get() + 1);
    Ok(())
}

/// `MO::import_from_reader( new POMO_FileReader( $filename ) )`'s pairs as `[original, translation]` arrays, or
/// false; `null` when WordPress's own code must run (the file cannot be read, or its table is empty, where
/// WordPress's reader throws).
fn mo_pairs(a: &[Value]) -> R {
    let Ok(bytes) = std::fs::read(path(a, 0)?) else {
        return Ok(Value::Null);
    };
    count_pomo()?;
    Ok(match speedyspec_wp_i18n_files::mo_pomo::mo_pairs(&bytes) {
        MoPairs::Pairs(pairs) => Value::Array(
            pairs
                .iter()
                .enumerate()
                .map(|(i, (o, t))| {
                    (
                        Key::Int(i as i64),
                        Value::Array(vec![
                            (Key::Int(0), bytes_value(o)),
                            (Key::Int(1), bytes_value(t)),
                        ]),
                    )
                })
                .collect(),
        ),
        MoPairs::False => Value::Bool(false),
        MoPairs::ZeroLengthRead => Value::Null,
    })
}

/// A value as PHP's string concatenation turns it into bytes; `None` for an array or object.
fn concat_bytes(v: &Value) -> Option<Vec<u8>> {
    Some(match v.read().as_ref() {
        Value::Str(s) => s.as_bytes().to_vec(),
        Value::Bytes(b) => b.clone(),
        Value::Null | Value::Bool(false) => Vec::new(),
        Value::Bool(true) => b"1".to_vec(),
        Value::Int(n) => n.to_string().into_bytes(),
        Value::Float(_) | Value::Array(_) | Value::Opaque(_) => return None,
    })
}

fn list_bytes(v: Option<&Value>) -> Option<Vec<Vec<u8>>> {
    match v?.read().as_ref() {
        Value::Array(items) => items.iter().map(|(_, v)| concat_bytes(v)).collect(),
        _ => None,
    }
}

/// `MO::export_to_file_handle()`'s bytes from the strings WordPress's methods gave; `null` when one is not a
/// scalar (WordPress's code then runs).
fn mo_pack(a: &[Value]) -> R {
    let (Some(originals), Some(translations), Some(headers)) = (
        list_bytes(a.first()),
        list_bytes(a.get(1)),
        a.get(2).and_then(concat_bytes),
    ) else {
        return Ok(Value::Null);
    };
    count_pomo()?;
    let pairs: Vec<(Vec<u8>, Vec<u8>)> = originals.into_iter().zip(translations).collect();
    Ok(bytes_value(&speedyspec_wp_i18n_files::mo_pomo::mo_pack(
        &pairs, &headers,
    )))
}

fn arg_bytes(a: &[Value], i: usize) -> Result<Vec<u8>, NativeError> {
    Ok(bytes(a, i)?.to_vec())
}

fn key_value(k: &PhpArrayKey) -> Key {
    match k {
        PhpArrayKey::Int(n) => Key::Int(*n),
        PhpArrayKey::Str(s) => Key::Str(s.clone()),
    }
}

/// `PO::import_from_file()`'s entries, each as the `Translation_Entry` properties `read_entry()` sets, and whether
/// the loop ended on false: `[entries, failed]`. `null` when the file cannot be read (WordPress's code then runs).
fn po_parse(a: &[Value]) -> R {
    let Ok(file) = std::fs::read(path(a, 0)?) else {
        return Ok(Value::Null);
    };
    count_pomo()?;
    let parsed = speedyspec_wp_i18n_files::po::parse_po(&file);
    let opt = |v: &Option<Vec<u8>>| v.as_deref().map_or(Value::Null, bytes_value);
    let list = |items: &[Vec<u8>]| {
        Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), bytes_value(v)))
                .collect(),
        )
    };
    let entries = parsed
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let field = |name: &str, v: Value| (Key::Str(name.to_owned()), v);
            let translations = Value::Array(
                e.translations
                    .iter()
                    .map(|(k, v)| (key_value(k), bytes_value(v)))
                    .collect(),
            );
            let fields = vec![
                field("context", opt(&e.context)),
                field("singular", opt(&e.singular)),
                field("plural", opt(&e.plural)),
                field("translations", translations),
                field("is_plural", Value::Bool(e.is_plural)),
                field("translator_comments", bytes_value(&e.translator_comments)),
                field("extracted_comments", bytes_value(&e.extracted_comments)),
                field("references", list(&e.references)),
                field("flags", list(&e.flags)),
            ];
            (Key::Int(i as i64), Value::Array(fields))
        })
        .collect();
    Ok(Value::Array(vec![
        (Key::Int(0), Value::Array(entries)),
        (Key::Int(1), Value::Bool(parsed.failed)),
    ]))
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
        f("plural_parse", plural_parse),
        f("mo_pairs", mo_pairs),
        f("mo_pack", mo_pack),
        f("po_parse", po_parse),
        f("po_poify", |a| {
            Ok(bytes_value(&po::poify(&arg_bytes(a, 0)?)))
        }),
        f("po_unpoify", |a| {
            Ok(bytes_value(&po::unpoify(&arg_bytes(a, 0)?)))
        }),
        f("po_prepend_each_line", |a| {
            Ok(bytes_value(&po::prepend_each_line(
                &arg_bytes(a, 0)?,
                &arg_bytes(a, 1)?,
            )))
        }),
        f("po_comment_block", |a| {
            Ok(bytes_value(&po::comment_block(
                &arg_bytes(a, 0)?,
                &arg_bytes(a, 1)?,
            )))
        }),
        f("po_match_newlines", |a| {
            Ok(bytes_value(&po::match_begin_and_end_newlines(
                &arg_bytes(a, 0)?,
                &arg_bytes(a, 1)?,
            )))
        }),
        f("po_trim_quotes", |a| {
            Ok(bytes_value(po::trim_quotes(&arg_bytes(a, 0)?)))
        }),
        f("pomo_stats", |_| Ok(Value::Int(state()?.pomo.get() as i64))),
        f("plural_execute", plural_execute),
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

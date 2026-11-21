//! Native `WP_Translation_File_MO::export()` and `WP_Translation_File_PHP::export()`: the object's own `$headers`
//! and `$entries` read as bytes (keys included, which may not be UTF-8) and written by the files crate's writers.
//! When a value is not a string, `null` is returned and the facade runs WordPress's PHP body.

use speedyspec_php::ext_php_rs::args::Arg;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::ext_php_rs::flags::DataType;
use speedyspec_php::ext_php_rs::types::{ArrayKey, Zval};
use speedyspec_php::ext_php_rs::zend::ExecuteData;
use speedyspec_wp_i18n_files::mo_pomo::{translation_file_mo, translation_file_php};

use crate::state::shared;
use crate::zv;

/// An array of strings as `(key bytes, value bytes)`.
type Strings = Vec<(Vec<u8>, Vec<u8>)>;

/// An array of strings as `(key bytes, value bytes)`; `None` when it is not an array of strings.
fn strings(z: &Zval) -> Option<Strings> {
    let array = z.dereference().array()?;
    let mut out = Vec::with_capacity(array.len());
    for (k, v) in array.iter() {
        let key = match k {
            ArrayKey::Long(n) => n.to_string().into_bytes(),
            ArrayKey::String(s) => s.into_bytes(),
            ArrayKey::Str(s) => s.as_bytes().to_vec(),
            ArrayKey::ZendString(s) => s.as_bytes().to_vec(),
        };
        out.push((key, v.dereference().zend_str()?.as_bytes().to_vec()));
    }
    Some(out)
}

fn count_export() {
    if let Some(s) = shared() {
        s.exports.set(s.exports.get() + 1);
    }
}

fn arrays(ex: &ExecuteData) -> Option<(Strings, Strings)> {
    let headers = strings(&zv::arg_copy(ex, 0)?)?;
    let entries = strings(&zv::arg_copy(ex, 1)?)?;
    Some((headers, entries))
}

extern "C" fn native_export_mo(ex: &mut ExecuteData, ret: &mut Zval) {
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (headers, entries) = arrays(ex)?;
        let big =
            zv::arg_copy(ex, 2).is_some_and(|u| u.zend_str().is_some_and(|s| s.as_bytes() == b"N"));
        Some(translation_file_mo(&headers, &entries, big))
    }))
    .unwrap_or(None);
    if let Some(bytes) = out {
        count_export();
        zv::return_value(ret, zv::string_zval(&bytes));
    }
}

extern "C" fn native_export_php(ex: &mut ExecuteData, ret: &mut Zval) {
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (headers, entries) = arrays(ex)?;
        Some(translation_file_php(&headers, &entries))
    }))
    .unwrap_or(None);
    if let Some(bytes) = out {
        count_export();
        zv::return_value(ret, zv::string_zval(&bytes));
    }
}

extern "C" fn native_export_stats(_ex: &mut ExecuteData, ret: &mut Zval) {
    ret.set_long(shared().map_or(0, |s| s.exports.get() as i64));
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    let ns = "SpeedySpec\\WpI18n\\Native\\";
    vec![
        FunctionBuilder::new(format!("{ns}file_export_mo"), native_export_mo)
            .arg(Arg::new("headers", DataType::Mixed))
            .arg(Arg::new("entries", DataType::Mixed))
            .arg(Arg::new("uint32", DataType::Mixed))
            .returns(DataType::Mixed, false, true),
        FunctionBuilder::new(format!("{ns}file_export_php"), native_export_php)
            .arg(Arg::new("headers", DataType::Mixed))
            .arg(Arg::new("entries", DataType::Mixed))
            .returns(DataType::Mixed, false, true),
        FunctionBuilder::new(format!("{ns}export_stats"), native_export_stats).returns(
            DataType::Long,
            false,
            false,
        ),
    ]
}

//! Duotone SVG filters: `get_svg_filters` and the markup of `WP_Duotone::get_filter_svg_from_preset` (the block
//! supports area keeps `WP_Duotone`; only what `get_svg_filters` needs is copied here).

mod colord;

pub use colord::{Rgba, colord_parse};

use speedyspec_hooks::Value;
use speedyspec_wp_theme_json_contracts::Origin;

use crate::ThemeJsonDeps;
use crate::array_path;
use crate::nodes::setting_nodes;
use crate::php::{float_to_string, to_php_string};
use crate::php_value::{empty, entries};

/// The filter after WordPress's whitespace clean-up (no `SCRIPT_DEBUG`), with `{id}` and the four tables.
const TEMPLATE: [&str; 6] = [
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 0 0" width="0" height="0" focusable="false" role="none" style="visibility: hidden; position: absolute; left: -9999px; overflow: hidden;" ><defs><filter id=""#,
    r#""><feColorMatrix color-interpolation-filters="sRGB" type="matrix" values=" .299 .587 .114 0 0 .299 .587 .114 0 0 .299 .587 .114 0 0 .299 .587 .114 0 0 " /><feComponentTransfer color-interpolation-filters="sRGB" ><feFuncR type="table" tableValues=""#,
    r#"" /><feFuncG type="table" tableValues=""#,
    r#"" /><feFuncB type="table" tableValues=""#,
    r#"" /><feFuncA type="table" tableValues=""#,
    r#"" /></feComponentTransfer><feComposite in2="SourceGraphic" operator="in" /></filter></defs></svg>"#,
];

/// `preg_replace( "/[\r\n\t ]+/", ' ', … )`, then `str_replace( '> <', '><', … )`, then `trim()`.
fn clean_whitespace(svg: &[u8]) -> Vec<u8> {
    let mut collapsed = Vec::with_capacity(svg.len());
    let mut in_space = false;
    for &b in svg {
        if matches!(b, b'\r' | b'\n' | b'\t' | b' ') {
            if !in_space {
                collapsed.push(b' ');
            }
            in_space = true;
        } else {
            collapsed.push(b);
            in_space = false;
        }
    }
    let mut out = Vec::with_capacity(collapsed.len());
    let mut i = 0;
    while i < collapsed.len() {
        if collapsed[i..].starts_with(b"> <") {
            out.extend_from_slice(b"><");
            i += 3;
        } else {
            out.push(collapsed[i]);
            i += 1;
        }
    }
    let is_trim = |b: &u8| b" \t\n\r\0\x0b".contains(b);
    let start = out.iter().position(|b| !is_trim(b)).unwrap_or(out.len());
    let end = out
        .iter()
        .rposition(|b| !is_trim(b))
        .map_or(start, |e| e + 1);
    out[start..end.max(start)].to_vec()
}

/// `WP_Duotone::get_filter_svg_from_preset( $preset )`, with its two `_deprecated_function` notices and the
/// `_doing_it_wrong` notice for colours it cannot parse.
pub fn filter_svg_from_preset(preset: &Value, deps: &ThemeJsonDeps) -> Vec<u8> {
    deps.diagnostics
        .deprecated_function("get_filter_svg_from_preset", "6.3.0", "");
    deps.diagnostics
        .deprecated_function("get_filter_id_from_preset", "6.3.0", "");
    let filter_id = match array_path::get(preset, &["slug"]).filter(|v| !matches!(v, Value::Null)) {
        Some(slug) => {
            let mut id = b"wp-duotone-".to_vec();
            id.extend(to_php_string(slug));
            id
        }
        None => Vec::new(),
    };
    let mut tables: [Vec<String>; 4] = Default::default();
    for (_, color) in entries(array_path::get(preset, &["colors"]).unwrap_or(&Value::Null)) {
        let text = String::from_utf8_lossy(&to_php_string(color)).into_owned();
        match colord_parse(&text) {
            None => {
                let template = deps.gettext.translate(
                    "\"%1$s\" in %2$s %3$s is not a hex or rgb string.",
                    "default",
                );
                let message = String::from_utf8_lossy(&template)
                    .replacen("%1$s", &text, 1)
                    .replacen("%2$s", "theme.json", 1)
                    .replacen("%3$s", "settings.color.duotone", 1);
                deps.diagnostics.doing_it_wrong(
                    "WP_Duotone::get_filter_svg",
                    message.as_bytes(),
                    "6.3.0",
                );
            }
            Some(rgba) => {
                tables[0].push(float_to_string(rgba.r / 255.0));
                tables[1].push(float_to_string(rgba.g / 255.0));
                tables[2].push(float_to_string(rgba.b / 255.0));
                tables[3].push(float_to_string(rgba.a));
            }
        }
    }
    let text = &*deps.text;
    let mut svg = TEMPLATE[0].as_bytes().to_vec();
    svg.extend(text.esc_attr(&filter_id));
    for (i, table) in tables.iter().enumerate() {
        svg.extend_from_slice(TEMPLATE[i + 1].as_bytes());
        svg.extend(text.esc_attr(table.join(" ").as_bytes()));
    }
    svg.extend_from_slice(TEMPLATE[5].as_bytes());
    clean_whitespace(&svg)
}

/// `get_svg_filters( $origins )`: every duotone preset's filter, node by node, origin by origin.
pub fn svg_filters(doc: &Value, meta: &Value, origins: &[Origin], deps: &ThemeJsonDeps) -> Vec<u8> {
    let mut filters = Vec::new();
    for metadata in setting_nodes(doc, meta) {
        let path: Vec<String> =
            entries(array_path::get(&metadata, &["path"]).unwrap_or(&Value::Null))
                .iter()
                .map(|(_, v)| String::from_utf8_lossy(&to_php_string(v)).into_owned())
                .collect();
        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
        let node = array_path::get(doc, &refs)
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let Some(presets) = array_path::get(&node, &["color", "duotone"]).filter(|v| !empty(v))
        else {
            continue;
        };
        for origin in origins {
            let Some(list) =
                array_path::get(presets, &[origin.as_str()]).filter(|v| !matches!(v, Value::Null))
            else {
                continue;
            };
            for (_, preset) in entries(list) {
                filters.extend(filter_svg_from_preset(preset, deps));
            }
        }
    }
    filters
}

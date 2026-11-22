//! The Data Mapper between WordPress's array form of a theme.json document and the typed model.
//!
//! Decoding never fails: a value without the shape a field expects becomes [`Leaf::Raw`], and keys the schema does
//! not name go to `extra`. Encoding writes struct fields in the schema's order, then the maps and `extra` in their
//! own order, so the result equals the input by PHP's `==`.

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::model::{
    BackgroundSettings, BackgroundStyles, BlockVisibilitySettings, BorderSettings, BorderStyles,
    ColorSettings, ColorStyles, CssValue, DimensionsSettings, DimensionsStyles, FilterStyles,
    LayoutSettings, LightboxSettings, OrderedMap, OutlineStyles, PhpString, PositionSettings,
    Preset, Presets, Scales, Settings, ShadowSettings, SpacingSettings, SpacingStyles, StyleNode,
    StyleProperties, StyleValue, ThemeJson, TypographySettings, TypographyStyles, ViewportSettings,
};
use speedyspec_wp_theme_json_contracts::{ByOrigin, Leaf, Origin};

/// Maps theme.json documents between WordPress's arrays and the typed model.
pub struct ThemeJsonCodec;

impl ThemeJsonCodec {
    /// Reads a document from WordPress's array form. Anything that is not an array is an empty document.
    pub fn decode(value: &Value) -> ThemeJson {
        ThemeJson::decode(value).unwrap_or_default()
    }

    /// Writes a document in WordPress's array form.
    pub fn encode(doc: &ThemeJson) -> Value {
        doc.encode()
    }
}

/// A model type with a PHP array form.
pub(crate) trait PhpShape: Sized {
    /// Reads the value, or `None` when it does not have this type's shape.
    fn decode(value: &Value) -> Option<Self>;
    /// Writes the value.
    fn encode(&self) -> Value;
}

impl<T: PhpShape> PhpShape for Leaf<T> {
    fn decode(value: &Value) -> Option<Self> {
        Some(
            T::decode(value)
                .map(Leaf::Typed)
                .unwrap_or_else(|| Leaf::Raw(value.clone())),
        )
    }

    fn encode(&self) -> Value {
        match self {
            Leaf::Typed(t) => t.encode(),
            Leaf::Raw(v) => v.clone(),
        }
    }
}

impl PhpShape for Value {
    fn decode(value: &Value) -> Option<Self> {
        Some(value.clone())
    }

    fn encode(&self) -> Value {
        self.clone()
    }
}

impl PhpShape for bool {
    fn decode(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn encode(&self) -> Value {
        Value::Bool(*self)
    }
}

impl PhpShape for i64 {
    fn decode(value: &Value) -> Option<Self> {
        match value {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    fn encode(&self) -> Value {
        Value::Int(*self)
    }
}

impl PhpShape for PhpString {
    fn decode(value: &Value) -> Option<Self> {
        match value {
            Value::Str(s) => Some(PhpString(s.as_bytes().to_vec())),
            Value::Bytes(b) => Some(PhpString(b.clone())),
            _ => None,
        }
    }

    fn encode(&self) -> Value {
        string_value(&self.0)
    }
}

/// A PHP string as the Hook API carries it: `Str` when it is UTF-8, `Bytes` otherwise.
pub(crate) fn string_value(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(s) => Value::Str(s.to_owned()),
        Err(_) => Value::Bytes(bytes.to_vec()),
    }
}

fn entries(value: &Value) -> Option<&[(Key, Value)]> {
    match value {
        Value::Array(e) => Some(e),
        _ => None,
    }
}

fn is_list(entries: &[(Key, Value)]) -> bool {
    entries
        .iter()
        .enumerate()
        .all(|(i, (k, _))| *k == Key::Int(i as i64))
}

fn str_key(key: &Key) -> Option<&str> {
    match key {
        Key::Str(s) => Some(s),
        Key::Int(_) => None,
    }
}

fn key(name: &str) -> Key {
    Key::Str(name.to_owned())
}

impl PhpShape for CssValue {
    fn decode(value: &Value) -> Option<Self> {
        match value {
            Value::Str(_) | Value::Bytes(_) => PhpString::decode(value).map(CssValue::Str),
            Value::Int(i) => Some(CssValue::Int(*i)),
            Value::Float(f) => Some(CssValue::Float(*f)),
            Value::Array(e) => match e.as_slice() {
                [(Key::Str(k), target @ (Value::Str(_) | Value::Bytes(_)))] if k == "ref" => {
                    PhpString::decode(target).map(CssValue::Ref)
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn encode(&self) -> Value {
        match self {
            CssValue::Str(s) => s.encode(),
            CssValue::Int(i) => Value::Int(*i),
            CssValue::Float(f) => Value::Float(*f),
            CssValue::Ref(r) => Value::Array(vec![(key("ref"), r.encode())]),
        }
    }
}

impl PhpShape for StyleValue {
    fn decode(value: &Value) -> Option<Self> {
        if let Some(css) = CssValue::decode(value) {
            return Some(StyleValue::Css(css));
        }
        OrderedMap::<String, Leaf<StyleValue>>::decode(value).map(StyleValue::Map)
    }

    fn encode(&self) -> Value {
        match self {
            StyleValue::Css(c) => c.encode(),
            StyleValue::Map(m) => m.encode(),
        }
    }
}

impl<T: PhpShape> PhpShape for Vec<T> {
    fn decode(value: &Value) -> Option<Self> {
        let e = entries(value)?;
        if !is_list(e) {
            return None;
        }
        e.iter().map(|(_, v)| T::decode(v)).collect()
    }

    fn encode(&self) -> Value {
        Value::Array(
            self.iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), v.encode()))
                .collect(),
        )
    }
}

impl<T: PhpShape> PhpShape for OrderedMap<String, T> {
    fn decode(value: &Value) -> Option<Self> {
        let e = entries(value)?;
        let mut out = OrderedMap::new();
        for (k, v) in e {
            out.insert(str_key(k)?.to_owned(), T::decode(v)?);
        }
        Some(out)
    }

    fn encode(&self) -> Value {
        Value::Array(self.iter().map(|(k, v)| (key(k), v.encode())).collect())
    }
}

impl PhpShape for Preset {
    fn decode(value: &Value) -> Option<Self> {
        let mut preset = Preset::default();
        for (k, v) in entries(value)? {
            match str_key(k) {
                Some("slug") => preset.slug = Leaf::decode(v),
                Some("name") => preset.name = Leaf::decode(v),
                _ => {
                    preset.extra.insert(k.clone(), v.clone());
                }
            }
        }
        Some(preset)
    }

    fn encode(&self) -> Value {
        let mut out = Vec::new();
        if let Some(slug) = &self.slug {
            out.push((key("slug"), slug.encode()));
        }
        if let Some(name) = &self.name {
            out.push((key("name"), name.encode()));
        }
        out.extend(self.extra.iter().map(|(k, v)| (k.clone(), v.clone())));
        Value::Array(out)
    }
}

/// Reads an array keyed only by origin names into a [`ByOrigin`], or `None`.
fn by_origin<T>(value: &Value, read: impl Fn(&Value) -> Option<T>) -> Option<ByOrigin<T>> {
    let e = entries(value)?;
    if e.is_empty() {
        return None;
    }
    let mut out = ByOrigin::default();
    for (k, v) in e {
        let origin = Origin::parse(str_key(k)?.as_bytes())?;
        out.set(origin, read(v)?);
    }
    // Encoding writes origins in WordPress's order; only accept data already in that order.
    let order: Vec<Origin> = e
        .iter()
        .filter_map(|(k, _)| str_key(k).and_then(|s| Origin::parse(s.as_bytes())))
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    (order == sorted).then_some(out)
}

fn encode_by_origin<T>(by: &ByOrigin<T>, write: impl Fn(&T) -> Value) -> Value {
    Value::Array(
        by.iter()
            .map(|(o, v)| (key(o.as_str()), write(v)))
            .collect(),
    )
}

impl PhpShape for Presets {
    fn decode(value: &Value) -> Option<Self> {
        let e = entries(value)?;
        if is_list(e) {
            return Vec::<Leaf<Preset>>::decode(value).map(Presets::Unkeyed);
        }
        by_origin(value, Leaf::<Vec<Leaf<Preset>>>::decode).map(Presets::ByOrigin)
    }

    fn encode(&self) -> Value {
        match self {
            Presets::Unkeyed(list) => list.encode(),
            Presets::ByOrigin(by) => encode_by_origin(by, PhpShape::encode),
        }
    }
}

impl PhpShape for Scales {
    fn decode(value: &Value) -> Option<Self> {
        entries(value)?;
        Some(
            by_origin(value, |v| Some(v.clone()))
                .map(Scales::ByOrigin)
                .unwrap_or_else(|| Scales::Unkeyed(value.clone())),
        )
    }

    fn encode(&self) -> Value {
        match self {
            Scales::Unkeyed(v) => v.clone(),
            Scales::ByOrigin(by) => encode_by_origin(by, Clone::clone),
        }
    }
}

/// Implements [`PhpShape`] for a `schema_object!` struct; the fields are listed in the order of its `KEYS`.
macro_rules! schema_object_shape {
    ($name:ident { $($field:ident),* $(,)? }) => {
        impl PhpShape for $name {
            fn decode(value: &Value) -> Option<Self> {
                const _: () = assert!([$(stringify!($field)),*].len() == $name::KEYS.len());
                let mut out = $name::default();
                'entries: for (k, v) in entries(value)? {
                    if let Some(name) = str_key(k) {
                        let mut index = 0usize;
                        $(
                            if $name::KEYS[index] == name {
                                out.$field = PhpShape::decode(v);
                                continue 'entries;
                            }
                            index += 1;
                        )*
                        let _ = index;
                    }
                    out.extra.insert(k.clone(), v.clone());
                }
                Some(out)
            }

            fn encode(&self) -> Value {
                let mut out = Vec::new();
                let mut index = 0usize;
                $(
                    if let Some(v) = &self.$field {
                        out.push((key($name::KEYS[index]), v.encode()));
                    }
                    index += 1;
                )*
                let _ = index;
                out.extend(self.extra.iter().map(|(k, v)| (k.clone(), v.clone())));
                Value::Array(out)
            }
        }
    };
}

schema_object_shape!(BackgroundSettings {
    background_image,
    background_size,
    gradient
});
schema_object_shape!(BorderSettings {
    color,
    radius,
    radius_sizes,
    style,
    width
});
schema_object_shape!(ColorSettings {
    background,
    custom,
    custom_duotone,
    custom_gradient,
    default_duotone,
    default_gradients,
    default_palette,
    duotone,
    gradients,
    link,
    heading,
    button,
    caption,
    palette,
    text
});
schema_object_shape!(DimensionsSettings {
    aspect_ratio,
    aspect_ratios,
    default_aspect_ratios,
    dimension_sizes,
    height,
    min_height,
    min_width,
    width
});
schema_object_shape!(LayoutSettings {
    content_size,
    wide_size,
    allow_editing,
    allow_custom_content_and_wide_size
});
schema_object_shape!(LightboxSettings {
    enabled,
    allow_editing
});
schema_object_shape!(PositionSettings { fixed, sticky });
schema_object_shape!(BlockVisibilitySettings { allow_editing });
schema_object_shape!(SpacingSettings {
    custom_spacing_size,
    default_spacing_sizes,
    spacing_sizes,
    spacing_scale,
    block_gap,
    margin,
    padding,
    units
});
schema_object_shape!(ShadowSettings {
    presets,
    default_presets
});
schema_object_shape!(TypographySettings {
    fluid,
    custom_font_size,
    default_font_sizes,
    drop_cap,
    font_families,
    font_sizes,
    font_style,
    font_weight,
    letter_spacing,
    line_height,
    text_align,
    text_columns,
    text_decoration,
    text_indent,
    text_transform,
    writing_mode
});
schema_object_shape!(ViewportSettings { mobile, tablet });
schema_object_shape!(Settings {
    appearance_tools,
    use_root_padding_aware_alignments,
    background,
    border,
    color,
    custom,
    dimensions,
    layout,
    lightbox,
    position,
    block_visibility,
    spacing,
    shadow,
    typography,
    viewport,
    blocks
});

schema_object_shape!(BackgroundStyles {
    background_image,
    background_position,
    background_repeat,
    background_size,
    background_attachment,
    gradient
});
schema_object_shape!(BorderStyles {
    color,
    radius,
    style,
    width,
    top,
    right,
    bottom,
    left
});
schema_object_shape!(ColorStyles {
    background,
    gradient,
    text
});
schema_object_shape!(DimensionsStyles {
    aspect_ratio,
    height,
    min_height,
    min_width,
    width
});
schema_object_shape!(FilterStyles { duotone });
schema_object_shape!(OutlineStyles {
    color,
    offset,
    style,
    width
});
schema_object_shape!(SpacingStyles {
    margin,
    padding,
    block_gap
});
schema_object_shape!(TypographyStyles {
    font_family,
    font_size,
    font_style,
    font_weight,
    letter_spacing,
    line_height,
    text_align,
    text_columns,
    text_decoration,
    text_indent,
    text_shadow,
    text_transform,
    writing_mode
});
schema_object_shape!(StyleProperties {
    background,
    border,
    color,
    dimensions,
    filter,
    outline,
    shadow,
    spacing,
    typography,
    css
});

impl PhpShape for StyleNode {
    fn decode(value: &Value) -> Option<Self> {
        let e = entries(value)?;
        let mut node = StyleNode::default();
        let mut own = Vec::new();
        for (k, v) in e {
            match str_key(k) {
                Some(name) if StyleProperties::KEYS.contains(&name) => {
                    own.push((k.clone(), v.clone()))
                }
                Some("elements") => node.elements = Leaf::decode(v),
                Some("blocks") => node.blocks = Leaf::decode(v),
                Some("variations") => node.variations = Leaf::decode(v),
                Some(name) if matches!(v, Value::Array(_)) => {
                    node.states.insert(name.to_owned(), Leaf::decode(v)?);
                }
                _ => own.push((k.clone(), v.clone())),
            }
        }
        node.properties = StyleProperties::decode(&Value::Array(own))?;
        Some(node)
    }

    fn encode(&self) -> Value {
        let Value::Array(mut out) = self.properties.encode() else {
            unreachable!()
        };
        let extra = out.split_off(out.len() - self.properties.extra.len());
        for (name, child) in [
            ("elements", &self.elements),
            ("blocks", &self.blocks),
            ("variations", &self.variations),
        ] {
            if let Some(child) = child {
                out.push((key(name), child.encode()));
            }
        }
        out.extend(self.states.iter().map(|(k, v)| (key(k), v.encode())));
        out.extend(extra);
        Value::Array(out)
    }
}

impl PhpShape for ThemeJson {
    fn decode(value: &Value) -> Option<Self> {
        let mut doc = ThemeJson::default();
        for (k, v) in entries(value)? {
            match str_key(k) {
                Some("blockTypes") => doc.block_types = Some(v.clone()),
                Some("customTemplates") => doc.custom_templates = Some(v.clone()),
                Some("description") => doc.description = Leaf::decode(v),
                Some("patterns") => doc.patterns = Some(v.clone()),
                Some("settings") => doc.settings = Leaf::decode(v),
                Some("slug") => doc.slug = Leaf::decode(v),
                Some("styles") => doc.styles = Leaf::decode(v),
                Some("templateParts") => doc.template_parts = Some(v.clone()),
                Some("title") => doc.title = Leaf::decode(v),
                Some("version") => doc.version = Leaf::decode(v),
                _ => {
                    doc.extra.insert(k.clone(), v.clone());
                }
            }
        }
        Some(doc)
    }

    fn encode(&self) -> Value {
        let mut out = Vec::new();
        let mut push = |name: &str, v: Option<Value>| {
            if let Some(v) = v {
                out.push((key(name), v));
            }
        };
        push("blockTypes", self.block_types.clone());
        push("customTemplates", self.custom_templates.clone());
        push(
            "description",
            self.description.as_ref().map(PhpShape::encode),
        );
        push("patterns", self.patterns.clone());
        push("settings", self.settings.as_ref().map(PhpShape::encode));
        push("slug", self.slug.as_ref().map(PhpShape::encode));
        push("styles", self.styles.as_ref().map(PhpShape::encode));
        push("templateParts", self.template_parts.clone());
        push("title", self.title.as_ref().map(PhpShape::encode));
        push("version", self.version.as_ref().map(PhpShape::encode));
        out.extend(self.extra.iter().map(|(k, v)| (k.clone(), v.clone())));
        Value::Array(out)
    }
}

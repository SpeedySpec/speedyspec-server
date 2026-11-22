use crate::Leaf;
use crate::model::{OrderedMap, StyleValue, schema_object};

schema_object! {
    /// `styles.background`.
    BackgroundStyles {
        /// `backgroundImage`.
        background_image: Leaf<StyleValue> = "backgroundImage",
        /// `backgroundPosition`.
        background_position: Leaf<StyleValue> = "backgroundPosition",
        /// `backgroundRepeat`.
        background_repeat: Leaf<StyleValue> = "backgroundRepeat",
        /// `backgroundSize`.
        background_size: Leaf<StyleValue> = "backgroundSize",
        /// `backgroundAttachment`.
        background_attachment: Leaf<StyleValue> = "backgroundAttachment",
        /// `gradient`.
        gradient: Leaf<StyleValue> = "gradient",
    }
}

schema_object! {
    /// `styles.border`.
    BorderStyles {
        /// `color`.
        color: Leaf<StyleValue> = "color",
        /// `radius`.
        radius: Leaf<StyleValue> = "radius",
        /// `style`.
        style: Leaf<StyleValue> = "style",
        /// `width`.
        width: Leaf<StyleValue> = "width",
        /// `top`.
        top: Leaf<StyleValue> = "top",
        /// `right`.
        right: Leaf<StyleValue> = "right",
        /// `bottom`.
        bottom: Leaf<StyleValue> = "bottom",
        /// `left`.
        left: Leaf<StyleValue> = "left",
    }
}

schema_object! {
    /// `styles.color`.
    ColorStyles {
        /// `background`.
        background: Leaf<StyleValue> = "background",
        /// `gradient`.
        gradient: Leaf<StyleValue> = "gradient",
        /// `text`.
        text: Leaf<StyleValue> = "text",
    }
}

schema_object! {
    /// `styles.dimensions`.
    DimensionsStyles {
        /// `aspectRatio`.
        aspect_ratio: Leaf<StyleValue> = "aspectRatio",
        /// `height`.
        height: Leaf<StyleValue> = "height",
        /// `minHeight`.
        min_height: Leaf<StyleValue> = "minHeight",
        /// `minWidth`.
        min_width: Leaf<StyleValue> = "minWidth",
        /// `width`.
        width: Leaf<StyleValue> = "width",
    }
}

schema_object! {
    /// `styles.filter`.
    FilterStyles {
        /// `duotone`.
        duotone: Leaf<StyleValue> = "duotone",
    }
}

schema_object! {
    /// `styles.outline`.
    OutlineStyles {
        /// `color`.
        color: Leaf<StyleValue> = "color",
        /// `offset`.
        offset: Leaf<StyleValue> = "offset",
        /// `style`.
        style: Leaf<StyleValue> = "style",
        /// `width`.
        width: Leaf<StyleValue> = "width",
    }
}

schema_object! {
    /// `styles.spacing`.
    SpacingStyles {
        /// `margin`.
        margin: Leaf<StyleValue> = "margin",
        /// `padding`.
        padding: Leaf<StyleValue> = "padding",
        /// `blockGap`.
        block_gap: Leaf<StyleValue> = "blockGap",
    }
}

schema_object! {
    /// `styles.typography`.
    TypographyStyles {
        /// `fontFamily`.
        font_family: Leaf<StyleValue> = "fontFamily",
        /// `fontSize`.
        font_size: Leaf<StyleValue> = "fontSize",
        /// `fontStyle`.
        font_style: Leaf<StyleValue> = "fontStyle",
        /// `fontWeight`.
        font_weight: Leaf<StyleValue> = "fontWeight",
        /// `letterSpacing`.
        letter_spacing: Leaf<StyleValue> = "letterSpacing",
        /// `lineHeight`.
        line_height: Leaf<StyleValue> = "lineHeight",
        /// `textAlign`.
        text_align: Leaf<StyleValue> = "textAlign",
        /// `textColumns`.
        text_columns: Leaf<StyleValue> = "textColumns",
        /// `textDecoration`.
        text_decoration: Leaf<StyleValue> = "textDecoration",
        /// `textIndent`.
        text_indent: Leaf<StyleValue> = "textIndent",
        /// `textShadow`.
        text_shadow: Leaf<StyleValue> = "textShadow",
        /// `textTransform`.
        text_transform: Leaf<StyleValue> = "textTransform",
        /// `writingMode`.
        writing_mode: Leaf<StyleValue> = "writingMode",
    }
}

schema_object! {
    /// The style properties one node may set (`VALID_STYLES`).
    StyleProperties {
        /// `background`.
        background: Leaf<BackgroundStyles> = "background",
        /// `border`.
        border: Leaf<BorderStyles> = "border",
        /// `color`.
        color: Leaf<ColorStyles> = "color",
        /// `dimensions`.
        dimensions: Leaf<DimensionsStyles> = "dimensions",
        /// `filter`.
        filter: Leaf<FilterStyles> = "filter",
        /// `outline`.
        outline: Leaf<OutlineStyles> = "outline",
        /// `shadow`.
        shadow: Leaf<StyleValue> = "shadow",
        /// `spacing`.
        spacing: Leaf<SpacingStyles> = "spacing",
        /// `typography`.
        typography: Leaf<TypographyStyles> = "typography",
        /// `css`.
        css: Leaf<StyleValue> = "css",
    }
}

/// One style node: the root `styles`, an element, a block, a variation, or a state of one of them.
///
/// WordPress nests the same shape at every level and lets the sanitizer decide which children a level may have,
/// so one recursive type serves them all. `states` holds the keys that are neither properties nor `elements`,
/// `blocks` or `variations`: pseudo-selectors (`:hover`), responsive breakpoint states and custom states, in the
/// data's order. Unknown keys stay in `properties.extra`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleNode {
    /// The node's own style properties.
    pub properties: StyleProperties,
    /// `elements`, by element name.
    pub elements: Option<Leaf<OrderedMap<String, Leaf<StyleNode>>>>,
    /// `blocks`, by block name.
    pub blocks: Option<Leaf<OrderedMap<String, Leaf<StyleNode>>>>,
    /// `variations`, by variation name.
    pub variations: Option<Leaf<OrderedMap<String, Leaf<StyleNode>>>>,
    /// Pseudo-selector, breakpoint and custom states, by key.
    pub states: OrderedMap<String, Leaf<StyleNode>>,
}

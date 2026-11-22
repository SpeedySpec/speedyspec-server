use crate::model::{OrderedMap, PhpString, Presets, Scales, schema_object};
use crate::{Leaf, Value};

schema_object! {
    /// `settings.background`.
    BackgroundSettings {
        /// `backgroundImage`.
        background_image: Leaf<bool> = "backgroundImage",
        /// `backgroundSize`.
        background_size: Leaf<bool> = "backgroundSize",
        /// `gradient`.
        gradient: Leaf<bool> = "gradient",
    }
}

schema_object! {
    /// `settings.border`.
    BorderSettings {
        /// `color`.
        color: Leaf<bool> = "color",
        /// `radius`.
        radius: Leaf<bool> = "radius",
        /// `radiusSizes`.
        radius_sizes: Leaf<Presets> = "radiusSizes",
        /// `style`.
        style: Leaf<bool> = "style",
        /// `width`.
        width: Leaf<bool> = "width",
    }
}

schema_object! {
    /// `settings.color`.
    ColorSettings {
        /// `background`.
        background: Leaf<bool> = "background",
        /// `custom`.
        custom: Leaf<bool> = "custom",
        /// `customDuotone`.
        custom_duotone: Leaf<bool> = "customDuotone",
        /// `customGradient`.
        custom_gradient: Leaf<bool> = "customGradient",
        /// `defaultDuotone`.
        default_duotone: Leaf<bool> = "defaultDuotone",
        /// `defaultGradients`.
        default_gradients: Leaf<bool> = "defaultGradients",
        /// `defaultPalette`.
        default_palette: Leaf<bool> = "defaultPalette",
        /// `duotone`.
        duotone: Leaf<Presets> = "duotone",
        /// `gradients`.
        gradients: Leaf<Presets> = "gradients",
        /// `link`.
        link: Leaf<bool> = "link",
        /// `heading`.
        heading: Leaf<bool> = "heading",
        /// `button`.
        button: Leaf<bool> = "button",
        /// `caption`.
        caption: Leaf<bool> = "caption",
        /// `palette`.
        palette: Leaf<Presets> = "palette",
        /// `text`.
        text: Leaf<bool> = "text",
    }
}

schema_object! {
    /// `settings.dimensions`.
    DimensionsSettings {
        /// `aspectRatio`.
        aspect_ratio: Leaf<bool> = "aspectRatio",
        /// `aspectRatios`.
        aspect_ratios: Leaf<Presets> = "aspectRatios",
        /// `defaultAspectRatios`.
        default_aspect_ratios: Leaf<bool> = "defaultAspectRatios",
        /// `dimensionSizes`.
        dimension_sizes: Leaf<Presets> = "dimensionSizes",
        /// `height`.
        height: Leaf<bool> = "height",
        /// `minHeight`.
        min_height: Leaf<bool> = "minHeight",
        /// `minWidth`.
        min_width: Leaf<bool> = "minWidth",
        /// `width`.
        width: Leaf<bool> = "width",
    }
}

schema_object! {
    /// `settings.layout`.
    LayoutSettings {
        /// `contentSize`.
        content_size: Leaf<PhpString> = "contentSize",
        /// `wideSize`.
        wide_size: Leaf<PhpString> = "wideSize",
        /// `allowEditing`.
        allow_editing: Leaf<bool> = "allowEditing",
        /// `allowCustomContentAndWideSize`.
        allow_custom_content_and_wide_size: Leaf<bool> = "allowCustomContentAndWideSize",
    }
}

schema_object! {
    /// `settings.lightbox`.
    LightboxSettings {
        /// `enabled`.
        enabled: Leaf<bool> = "enabled",
        /// `allowEditing`.
        allow_editing: Leaf<bool> = "allowEditing",
    }
}

schema_object! {
    /// `settings.position`.
    PositionSettings {
        /// `fixed`.
        fixed: Leaf<bool> = "fixed",
        /// `sticky`.
        sticky: Leaf<bool> = "sticky",
    }
}

schema_object! {
    /// `settings.blockVisibility`.
    BlockVisibilitySettings {
        /// `allowEditing`.
        allow_editing: Leaf<bool> = "allowEditing",
    }
}

schema_object! {
    /// `settings.spacing`.
    SpacingSettings {
        /// `customSpacingSize`.
        custom_spacing_size: Leaf<bool> = "customSpacingSize",
        /// `defaultSpacingSizes`.
        default_spacing_sizes: Leaf<bool> = "defaultSpacingSizes",
        /// `spacingSizes`.
        spacing_sizes: Leaf<Presets> = "spacingSizes",
        /// `spacingScale`.
        spacing_scale: Leaf<Scales> = "spacingScale",
        /// `blockGap`.
        block_gap: Leaf<bool> = "blockGap",
        /// `margin`.
        margin: Leaf<bool> = "margin",
        /// `padding`.
        padding: Leaf<bool> = "padding",
        /// `units`.
        units: Leaf<Vec<Leaf<PhpString>>> = "units",
    }
}

schema_object! {
    /// `settings.shadow`.
    ShadowSettings {
        /// `presets`.
        presets: Leaf<Presets> = "presets",
        /// `defaultPresets`.
        default_presets: Leaf<bool> = "defaultPresets",
    }
}

schema_object! {
    /// `settings.typography`. `fluid` accepts a boolean or an object of fluid settings; an object is kept as given.
    TypographySettings {
        /// `fluid`.
        fluid: Leaf<bool> = "fluid",
        /// `customFontSize`.
        custom_font_size: Leaf<bool> = "customFontSize",
        /// `defaultFontSizes`.
        default_font_sizes: Leaf<bool> = "defaultFontSizes",
        /// `dropCap`.
        drop_cap: Leaf<bool> = "dropCap",
        /// `fontFamilies`.
        font_families: Leaf<Presets> = "fontFamilies",
        /// `fontSizes`.
        font_sizes: Leaf<Presets> = "fontSizes",
        /// `fontStyle`.
        font_style: Leaf<bool> = "fontStyle",
        /// `fontWeight`.
        font_weight: Leaf<bool> = "fontWeight",
        /// `letterSpacing`.
        letter_spacing: Leaf<bool> = "letterSpacing",
        /// `lineHeight`.
        line_height: Leaf<bool> = "lineHeight",
        /// `textAlign`.
        text_align: Leaf<bool> = "textAlign",
        /// `textColumns`.
        text_columns: Leaf<bool> = "textColumns",
        /// `textDecoration`.
        text_decoration: Leaf<bool> = "textDecoration",
        /// `textIndent`.
        text_indent: Leaf<bool> = "textIndent",
        /// `textTransform`.
        text_transform: Leaf<bool> = "textTransform",
        /// `writingMode`.
        writing_mode: Leaf<bool> = "writingMode",
    }
}

schema_object! {
    /// `settings.viewport`: the responsive breakpoints.
    ViewportSettings {
        /// `mobile`.
        mobile: Leaf<PhpString> = "mobile",
        /// `tablet`.
        tablet: Leaf<PhpString> = "tablet",
    }
}

schema_object! {
    /// `settings`, at the root or for one block.
    Settings {
        /// `appearanceTools`.
        appearance_tools: Leaf<bool> = "appearanceTools",
        /// `useRootPaddingAwareAlignments`.
        use_root_padding_aware_alignments: Leaf<bool> = "useRootPaddingAwareAlignments",
        /// `background`.
        background: Leaf<BackgroundSettings> = "background",
        /// `border`.
        border: Leaf<BorderSettings> = "border",
        /// `color`.
        color: Leaf<ColorSettings> = "color",
        /// `custom`.
        custom: Value = "custom",
        /// `dimensions`.
        dimensions: Leaf<DimensionsSettings> = "dimensions",
        /// `layout`.
        layout: Leaf<LayoutSettings> = "layout",
        /// `lightbox`.
        lightbox: Leaf<LightboxSettings> = "lightbox",
        /// `position`.
        position: Leaf<PositionSettings> = "position",
        /// `blockVisibility`.
        block_visibility: Leaf<BlockVisibilitySettings> = "blockVisibility",
        /// `spacing`.
        spacing: Leaf<SpacingSettings> = "spacing",
        /// `shadow`.
        shadow: Leaf<ShadowSettings> = "shadow",
        /// `typography`.
        typography: Leaf<TypographySettings> = "typography",
        /// `viewport`.
        viewport: Leaf<ViewportSettings> = "viewport",
        /// `blocks`.
        blocks: Leaf<OrderedMap<String, Leaf<Settings>>> = "blocks",
    }
}


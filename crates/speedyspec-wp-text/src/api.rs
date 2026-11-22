//! `TextApi`: the content filters with WordPress's hooks, settings and statics around them.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_hooks::{Key, Value as HookValue};
use speedyspec_wp_hooks::{Count, HookApi, HookName};
use speedyspec_wp_shortcode_contracts::ShortcodeTagNames;
use speedyspec_wp_kses::KsesApi;
use speedyspec_wp_text_contracts::{
    Diagnostics, ErrorLevel, LinkContext, Locale, MimeTypes, RandomSource, SiteSettings, Smilies, TextError,
    TexturizeOverrides, Translator, WordCountType,
};
use speedyspec_wp_text_escape::{Charset, EscapeApi};

use crate::autop::AutopOptions;
use crate::autop_fast::wpautop;
use crate::balance::force_balance_tags;
use crate::small::{DEFAULT_SPACES, capital_p_dangit, convert_chars, ent2ncr, format_for_editor_core};
use crate::strings::{StrReplace, bytes_value, to_php_string};
use crate::strip::wp_strip_all_tags_core;
use crate::texturize::{HandTexturizer, TexturizeScope, TexturizeTables, Texturizer};
use crate::trim::{wp_html_excerpt, wp_trim_words_core};
use crate::unautop::Unautop;

pub use speedyspec_wp_text_escape::text_error;

/// Everything `TextApi` reads from outside: the hooks and the ports.
pub struct TextDeps {
    pub hooks: Rc<HookApi>,
    pub settings: Rc<dyn SiteSettings>,
    pub translator: Rc<dyn Translator>,
    pub diagnostics: Rc<dyn Diagnostics>,
    pub texturize_overrides: Rc<dyn TexturizeOverrides>,
    pub shortcode_tags: Rc<dyn ShortcodeTagNames>,
    /// The escapers (`esc_attr`, `esc_html`, ...), whose filters fire there.
    pub escape: Rc<EscapeApi>,
    /// kses: `wp_allowed_protocols()`, the URI attribute list and the entity names.
    pub kses: Rc<KsesApi>,
    pub locale: Rc<dyn Locale>,
    pub mime_types: Rc<dyn MimeTypes>,
    pub random: Rc<dyn RandomSource>,
    pub smilies: Rc<dyn Smilies>,
    pub link_context: Rc<dyn LinkContext>,
}

/// How `wpautop()` is run; `TextApi` takes the reference or a faster equivalent.
pub type AutopFn = for<'a> fn(&'a [u8], AutopOptions) -> Cow<'a, [u8]>;

/// What WordPress keeps in function statics, kept here for the life of the `TextApi`.
struct Statics {
    /// `wp_spaces_regexp()`'s `$spaces`, once non-empty.
    spaces: Option<Vec<u8>>,
    /// `capital_P_dangit()`'s `$dblq`, and its replacement built once.
    opening_double_quote: Option<(Vec<u8>, Rc<StrReplace>)>,
    /// `wptexturize()`'s tables, built on the first non-empty call and on `$reset`.
    tables: Option<Rc<TexturizeTables>>,
    /// `wptexturize()`'s `$run_texturize`: once a filter turns it off, it stays off.
    run_texturize: bool,
}

/// The hook names `TextApi` fires, hashed once.
struct Names {
    run_wptexturize: HookName,
    no_texturize_tags: HookName,
    no_texturize_shortcodes: HookName,
    wp_spaces_regexp: HookName,
    wp_trim_words: HookName,
    pre_ent2ncr: HookName,
    format_for_editor: HookName,
}

/// The facade over the content filters: one method per WordPress function that fires a filter, reads a setting
/// or a translation, reports a misuse, or keeps a static.
///
/// Functions with none of those (`normalize_whitespace`, `wp_replace_in_html_tags`, `strip_tags`, ...) have no
/// method; call them directly.
pub struct TextApi {
    pub(crate) d: TextDeps,
    /// Hook names made once each (making one copies its name).
    hook_names: RefCell<Vec<(&'static str, Rc<HookName>)>>,
    /// The smiley matcher for the last search seen (Lazy Load, like the texturize tables).
    pub(crate) smiley_engine: RefCell<Option<(Vec<u8>, Rc<crate::smilies::SmileyEngine>)>>,
    texturizer: Rc<dyn Texturizer>,
    autop: AutopFn,
    unautop: Unautop,
    statics: RefCell<Statics>,
    names: Names,
}

/// PHP's `empty()` on a hook value.
fn php_empty(value: &HookValue) -> bool {
    match value {
        HookValue::Null | HookValue::Bool(false) => true,
        HookValue::Int(i) => *i == 0,
        HookValue::Float(f) => *f == 0.0,
        HookValue::Str(s) => s.is_empty() || s == "0",
        HookValue::Bytes(b) => b.is_empty() || b == b"0",
        HookValue::Array(a) => a.is_empty(),
        HookValue::Bool(true) | HookValue::Opaque(_) => false,
    }
}

/// A filtered list as `in_array()` reads it: string entries only, and nothing for a non-array.
fn string_list(value: &HookValue) -> Vec<Vec<u8>> {
    match value {
        HookValue::Array(items) => items
            .iter()
            .filter_map(|(_, v)| match v {
                HookValue::Str(s) => Some(s.as_bytes().to_vec()),
                HookValue::Bytes(b) => Some(b.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn php_list(items: &[&str]) -> HookValue {
    HookValue::Array(items.iter().enumerate().map(|(i, s)| (Key::Int(i as i64), HookValue::Str((*s).into()))).collect())
}

impl TextApi {
    /// A facade on `deps`, with the hand-written texturizer and `wpautop`.
    ///
    /// @param deps
    ///     The hooks and ports.
    /// @return
    ///     The facade.
    pub fn new(deps: TextDeps) -> TextApi {
        TextApi {
            d: deps,
            hook_names: RefCell::new(Vec::new()),
            smiley_engine: RefCell::new(None),
            texturizer: Rc::new(HandTexturizer::default()),
            autop: wpautop,
            unautop: Unautop::new(),
            statics: RefCell::new(Statics {
                spaces: None,
                opening_double_quote: None,
                tables: None,
                run_texturize: true,
            }),
            names: Names {
                run_wptexturize: HookName::from("run_wptexturize"),
                no_texturize_tags: HookName::from("no_texturize_tags"),
                no_texturize_shortcodes: HookName::from("no_texturize_shortcodes"),
                wp_spaces_regexp: HookName::from("wp_spaces_regexp"),
                wp_trim_words: HookName::from("wp_trim_words"),
                pre_ent2ncr: HookName::from("pre_ent2ncr"),
                format_for_editor: HookName::from("format_for_editor"),
            },
        }
    }

    /// The same facade with another texturizer.
    ///
    /// @param texturizer
    ///     It must give the reference's bytes.
    /// @return
    ///     The facade.
    pub fn with_texturizer(mut self, texturizer: Rc<dyn Texturizer>) -> TextApi {
        self.texturizer = texturizer;
        self
    }

    /// The same facade with another `wpautop()`.
    ///
    /// @param autop
    ///     It must give the reference's bytes.
    /// @return
    ///     The facade.
    pub fn with_autop(mut self, autop: AutopFn) -> TextApi {
        self.autop = autop;
        self
    }

    /// Forgets what WordPress keeps for one request in function statics: `wptexturize()`'s tables and
    /// `$run_texturize`, `wp_spaces_regexp()`'s `$spaces`, `capital_P_dangit()`'s `$dblq`, and the smiley matcher
    /// built for the last search.
    pub fn new_request(&self) {
        *self.statics.borrow_mut() = Statics { spaces: None, opening_double_quote: None, tables: None, run_texturize: true };
        *self.smiley_engine.borrow_mut() = None;
    }

    /// The hooks and ports.
    pub fn deps(&self) -> &TextDeps {
        &self.d
    }

    /// A hook name, made once.
    pub(crate) fn hook(&self, name: &'static str) -> Rc<HookName> {
        if let Some((_, hook)) = self.hook_names.borrow().iter().find(|(n, _)| *n == name) {
            return hook.clone();
        }
        let hook = Rc::new(HookName::from(name));
        self.hook_names.borrow_mut().push((name, hook.clone()));
        hook
    }

    /// `apply_filters()`, through the fast path when nothing listens.
    pub(crate) fn filter(
        &self,
        name: &HookName,
        value: HookValue,
        args: impl FnOnce() -> Vec<HookValue>,
    ) -> Result<HookValue, TextError> {
        if self.d.hooks.skip_if_empty(name, Count::Filters).map_err(text_error)? {
            return Ok(value);
        }
        let out = self.d.hooks.apply_filters(name, value, args()).map_err(text_error)?;
        Ok(out.read().into_owned())
    }

    fn translate_x(&self, text: &str, context: &str) -> Result<String, TextError> {
        self.d.translator.translate_with_context(text, context, "default")
    }

    /// `wp_spaces_regexp()`: the fragment that counts as a space, filtered once (again while the filter returns an
    /// empty value, as PHP's `empty( $spaces )` does).
    ///
    /// @return
    ///     The fragment.
    pub fn wp_spaces_regexp(&self) -> Result<Vec<u8>, TextError> {
        if let Some(spaces) = &self.statics.borrow().spaces {
            return Ok(spaces.clone());
        }
        let value = self.filter(&self.names.wp_spaces_regexp, bytes_value(DEFAULT_SPACES.to_vec()), Vec::new)?;
        let spaces = to_php_string(&value).unwrap_or_default();
        if !php_empty(&value) {
            self.statics.borrow_mut().spaces = Some(spaces.clone());
        }
        Ok(spaces)
    }

    fn build_tables(&self) -> Result<Option<Rc<TexturizeTables>>, TextError> {
        let run = self.filter(&self.names.run_wptexturize, HookValue::Bool(true), Vec::new)?;
        if run == HookValue::Bool(false) {
            self.statics.borrow_mut().run_texturize = false;
            return Ok(None);
        }
        let failure = RefCell::new(None);
        let translate = |text: &str, context: &str| {
            self.translate_x(text, context).unwrap_or_else(|e| {
                *failure.borrow_mut() = Some(e);
                text.to_string()
            })
        };
        let cockney = self.d.texturize_overrides.cockney()?;
        let spaces = self.wp_spaces_regexp()?;
        let tables = TexturizeTables::build(&translate, cockney, &spaces);
        if let Some(e) = failure.into_inner() {
            return Err(e);
        }
        let tables = match tables {
            Ok(tables) => Rc::new(tables),
            // A spaces fragment that doesn't compile: PHP's preg_* would fail on every call.
            Err(e) => return Err(TextError::message(e.to_string())),
        };
        self.statics.borrow_mut().tables = Some(tables.clone());
        Ok(Some(tables))
    }

    /// `wptexturize( $text, $reset )`.
    ///
    /// @param text
    ///     The text; a PHP-empty value (including `"0"`) is returned as it is, before anything is built.
    /// @param reset
    ///     Rebuild the tables (firing `run_wptexturize` and translating again).
    /// @return
    ///     The texturized text; the input unchanged once `run_wptexturize` has returned `false`.
    pub fn wptexturize(&self, text: &HookValue, reset: bool) -> Result<HookValue, TextError> {
        if php_empty(text) || !self.statics.borrow().run_texturize {
            return Ok(text.clone());
        }
        let existing = self.statics.borrow().tables.clone();
        let tables = match existing {
            Some(tables) if !reset => tables,
            _ => match self.build_tables()? {
                Some(tables) => tables,
                None => return Ok(text.clone()),
            },
        };
        let no_tags = self.filter(&self.names.no_texturize_tags, php_list(&["pre", "code", "kbd", "style", "script", "tt"]), Vec::new)?;
        let no_shortcodes = self.filter(&self.names.no_texturize_shortcodes, php_list(&["code"]), Vec::new)?;
        let bytes: Cow<'_, [u8]> = match text {
            HookValue::Str(s) => Cow::Borrowed(s.as_bytes()),
            HookValue::Bytes(b) => Cow::Borrowed(b),
            other => match to_php_string(other) {
                Some(converted) => Cow::Owned(converted),
                None => return Ok(text.clone()),
            },
        };
        let no_tags = string_list(&no_tags);
        let no_shortcodes = string_list(&no_shortcodes);
        let tags = self.d.shortcode_tags.tag_names();
        let scope = TexturizeScope {
            no_texturize_tags: &no_tags,
            no_texturize_shortcodes: &no_shortcodes,
            shortcode_tags: &tags,
        };
        match self.texturizer.texturize(&bytes, &tables, &scope) {
            Ok(Cow::Borrowed(_)) => Ok(text.clone()),
            Ok(Cow::Owned(out)) => Ok(bytes_value(out)),
            Err(e) => Err(TextError::message(e.to_string())),
        }
    }

    /// `wpautop( $text, $br )`.
    ///
    /// @param text
    ///     The text.
    /// @param br
    ///     Whether single line breaks become `<br />`.
    /// @return
    ///     The text with paragraphs.
    pub fn wpautop<'a>(&self, text: &'a [u8], br: bool) -> Result<Cow<'a, [u8]>, TextError> {
        Ok((self.autop)(text, AutopOptions { br }))
    }

    /// `shortcode_unautop( $text )`, with the registered shortcodes.
    ///
    /// @param text
    ///     The text.
    /// @return
    ///     The text without `<p>` around lone shortcodes.
    pub fn shortcode_unautop<'a>(&self, text: &'a [u8]) -> Result<Cow<'a, [u8]>, TextError> {
        // Every match has a `[`. Without one nothing can change, and once the spaces fragment is cached the rest
        // of the call has no effect either (reading the tag names is a pure query), so it can be skipped.
        if memchr::memchr(b'[', text).is_none() && self.statics.borrow().spaces.is_some() {
            return Ok(Cow::Borrowed(text));
        }
        let tags = self.d.shortcode_tags.tag_names();
        if tags.is_empty() {
            return Ok(Cow::Borrowed(text));
        }
        let spaces = self.wp_spaces_regexp()?;
        Ok(self.unautop.run(text, &tags, self.d.shortcode_tags.generation(), &spaces))
    }

    /// `capital_P_dangit( $text )`, reading the current filter.
    ///
    /// @param text
    ///     The text.
    /// @return
    ///     The corrected text.
    pub fn capital_p_dangit<'a>(&self, text: &'a [u8]) -> Result<Cow<'a, [u8]>, TextError> {
        let current = self.d.hooks.current_filter();
        let in_title = matches!(&current, Some(HookValue::Str(f)) if f == "the_title" || f == "wp_title");
        if in_title {
            return Ok(capital_p_dangit(text, true, b""));
        }
        // `static $dblq`: translated on the first call, as WordPress does, whatever the text.
        let cached = self.statics.borrow().opening_double_quote.as_ref().map(|(_, r)| r.clone());
        let replace = match cached {
            Some(replace) => replace,
            None => {
                let quote = self.translate_x("&#8220;", "opening curly double quote")?.into_bytes();
                let replace = Rc::new(crate::small::capital_p_dangit_replace(&quote));
                self.statics.borrow_mut().opening_double_quote = Some((quote, replace.clone()));
                replace
            }
        };
        if memchr::memmem::find(text, b"Wordpress").is_none() {
            return Ok(Cow::Borrowed(text));
        }
        Ok(replace.apply(text))
    }

    /// `convert_chars( $content, $deprecated )`.
    ///
    /// @param content
    ///     The text.
    /// @param deprecated
    ///     The deprecated second argument; a non-empty one is reported.
    /// @return
    ///     The converted text.
    pub fn convert_chars<'a>(&self, content: &'a [u8], deprecated: &HookValue) -> Result<Cow<'a, [u8]>, TextError> {
        if !php_empty(deprecated) {
            self.d.diagnostics.deprecated_argument("convert_chars", "0.71", "")?;
        }
        Ok(convert_chars(content))
    }

    /// `balanceTags( $text, $force )`.
    ///
    /// @param text
    ///     The HTML.
    /// @param force
    ///     Balance even when the `use_balanceTags` option is off.
    /// @return
    ///     The HTML, balanced when forced or when the option is on.
    pub fn balance_tags<'a>(&self, text: &'a [u8], force: bool) -> Result<Cow<'a, [u8]>, TextError> {
        if force || self.d.settings.use_balance_tags()? {
            return Ok(force_balance_tags(text));
        }
        Ok(Cow::Borrowed(text))
    }

    /// `ent2ncr( $text )`, with its `pre_ent2ncr` short circuit.
    ///
    /// @param text
    ///     The text.
    /// @return
    ///     A non-null `pre_ent2ncr` result as it is, else the converted text.
    pub fn ent2ncr(&self, text: &[u8]) -> Result<HookValue, TextError> {
        let filtered = self.filter(&self.names.pre_ent2ncr, HookValue::Null, || vec![bytes_value(text.to_vec())])?;
        if filtered != HookValue::Null {
            return Ok(filtered);
        }
        Ok(bytes_value(ent2ncr(text).into_owned()))
    }

    /// `wp_strip_all_tags( $text, $remove_breaks )`.
    ///
    /// @param text
    ///     The value: `null` gives `""`, a scalar is converted, anything else is reported and gives `""`.
    /// @param remove_breaks
    ///     Whether runs of whitespace become one space.
    /// @return
    ///     The plain text.
    pub fn wp_strip_all_tags(&self, text: &HookValue, remove_breaks: bool) -> Result<Vec<u8>, TextError> {
        if *text == HookValue::Null {
            return Ok(Vec::new());
        }
        let Some(bytes) = to_php_string(text) else {
            let kind = if matches!(text, HookValue::Array(_)) { "array" } else { "object" };
            let template = self
                .d
                .translator
                .translate("Warning: %1$s expects parameter %2$s (%3$s) to be a %4$s, %5$s given.", "default")?;
            let message = template
                .replace("%1$s", "wp_strip_all_tags")
                .replace("%2$s", "#1")
                .replace("%3$s", "$text")
                .replace("%4$s", "string")
                .replace("%5$s", kind);
            self.d.diagnostics.trigger_error("", &message, ErrorLevel::Warning)?;
            return Ok(Vec::new());
        };
        Ok(wp_strip_all_tags_core(&bytes, remove_breaks).into_owned())
    }

    /// `wp_trim_words( $text, $num_words, $more )`.
    ///
    /// @param text
    ///     The text.
    /// @param num_words
    ///     How many words (or characters) to keep.
    /// @param more
    ///     What to append when cut; `None` is `__( '&hellip;' )`.
    /// @return
    ///     What the `wp_trim_words` filter returns.
    pub fn wp_trim_words(&self, text: &[u8], num_words: i64, more: Option<&[u8]>) -> Result<HookValue, TextError> {
        let more = match more {
            Some(more) => more.to_vec(),
            None => self.d.translator.translate("&hellip;", "default")?.into_bytes(),
        };
        let counts_characters = !matches!(self.d.settings.word_count_type()?, WordCountType::Words);
        let count_characters = counts_characters && {
            let charset = self.d.settings.blog_charset()?.to_ascii_lowercase();
            charset == "utf-8" || charset == "utf8"
        };
        let trimmed = wp_trim_words_core(text, num_words, &more, count_characters);
        self.filter(&self.names.wp_trim_words, bytes_value(trimmed), || {
            vec![HookValue::Int(num_words), bytes_value(more.clone()), bytes_value(text.to_vec())]
        })
    }

    /// `wp_html_excerpt( $str, $count, $more )`.
    ///
    /// @param text
    ///     The HTML.
    /// @param count
    ///     How many characters to keep.
    /// @param more
    ///     What to append when cut; `None` is `""`.
    /// @return
    ///     The excerpt.
    pub fn wp_html_excerpt(&self, text: &[u8], count: i64, more: Option<&[u8]>) -> Result<Vec<u8>, TextError> {
        Ok(wp_html_excerpt(text, count, more.unwrap_or_default()))
    }

    /// `format_for_editor( $text, $default_editor )`.
    ///
    /// @param text
    ///     The text.
    /// @param default_editor
    ///     The user's default editor, passed to the filter.
    /// @return
    ///     What the `format_for_editor` filter returns.
    pub fn format_for_editor(&self, text: &[u8], default_editor: &HookValue) -> Result<HookValue, TextError> {
        let charset = Charset::from_php_name(&self.d.settings.blog_charset()?);
        let formatted = format_for_editor_core(text, &charset).into_owned();
        self.filter(&self.names.format_for_editor, bytes_value(formatted), || vec![default_editor.clone()])
    }
}

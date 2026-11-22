//! `TextApi`'s smileys: `convert_smilies()` and `translate_smiley()`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::LazyLock;

use speedyspec_wp_text_contracts::TextError;

use crate::api::TextApi;
use crate::preg::{Pattern, preg_match};
use crate::smilies::{SmileyEngine, convert_smilies_core};
use crate::strings::{PHP_TRIM_DEFAULT, bytes_value, php_trim, to_php_string};

/// `translate_smiley()`'s extension pattern.
static EXTENSION: LazyLock<Pattern> = LazyLock::new(|| Pattern::php("/\\.([^.]+)$/"));

/// The image extensions `translate_smiley()` turns into an `<img>`.
const IMAGE_EXTENSIONS: [&[u8]; 7] = [b"jpg", b"jpeg", b"jpe", b"gif", b"png", b"webp", b"avif"];

impl TextApi {
    /// `translate_smiley( $matches )`.
    ///
    /// @param matched
    ///     The matched smiley (trimmed before lookup, as WordPress does).
    /// @return
    ///     An `<img>` for an image smiley, otherwise its text (an emoji); `""` for a smiley the table lacks.
    pub fn translate_smiley(&self, matched: &[u8]) -> Result<Vec<u8>, TextError> {
        let smiley = php_trim(matched, PHP_TRIM_DEFAULT);
        let Some(img) = self.d.smilies.image(smiley)? else {
            return Ok(Vec::new());
        };
        let ext = match preg_match(&EXTENSION, &img) {
            Ok(Some(m)) => Some(m.get(1).unwrap_or_default().to_ascii_lowercase()),
            _ => None,
        };
        if !ext.is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.as_slice())) {
            return Ok(img);
        }
        let src = self.d.smilies.includes_url(&[&b"images/smilies/"[..], &img].concat())?;
        let site = self.d.smilies.site_url()?;
        let src = self.filter(&self.hook("smilies_src"), bytes_value(src), || vec![bytes_value(img.clone()), bytes_value(site)])?;
        let src = to_php_string(&src).unwrap_or_default();
        let url = to_php_string(&self.esc_url(&src, None, b"display")?).unwrap_or_default();
        let alt = to_php_string(&self.d.escape.esc_attr(smiley)?).unwrap_or_default();
        Ok([
            &b"<img src=\""[..],
            &url,
            b"\" alt=\"",
            &alt,
            b"\" class=\"wp-smiley\" style=\"height: 1em; max-height: 1em;\" />",
        ]
        .concat())
    }

    /// The matcher for the current search, built once per search.
    fn smiley_engine(&self, search: &[u8]) -> Result<Option<Rc<SmileyEngine>>, TextError> {
        if let Some((cached, engine)) = &*self.smiley_engine.borrow()
            && cached == search
        {
            return Ok(Some(engine.clone()));
        }
        let smileys = self.d.smilies.smileys()?;
        let Some(engine) = SmileyEngine::new(search, &smileys).map(Rc::new) else {
            return Ok(None);
        };
        *self.smiley_engine.borrow_mut() = Some((search.to_vec(), engine.clone()));
        Ok(Some(engine))
    }

    /// `convert_smilies( $text )`.
    ///
    /// @param text
    ///     The text.
    /// @return
    ///     The text with smileys replaced; borrowed when smileys are off or there are none.
    pub fn convert_smilies<'a>(&self, text: &'a [u8]) -> Result<Cow<'a, [u8]>, TextError> {
        if !self.d.settings.use_smilies()? {
            return Ok(Cow::Borrowed(text));
        }
        let Some(search) = self.d.smilies.search()?.filter(|s| !s.is_empty()) else {
            return Ok(Cow::Borrowed(text));
        };
        let Some(engine) = self.smiley_engine(&search)? else {
            // PHP warns and preg_replace_callback returns null for a search PCRE rejects.
            return Ok(Cow::Borrowed(text));
        };
        let failure: RefCell<Option<TextError>> = RefCell::new(None);
        let out = convert_smilies_core(text, &engine, &mut |m| {
            self.translate_smiley(m).unwrap_or_else(|e| {
                failure.borrow_mut().get_or_insert(e);
                Vec::new()
            })
        });
        match failure.into_inner() {
            Some(e) => Err(e),
            None => Ok(out),
        }
    }
}

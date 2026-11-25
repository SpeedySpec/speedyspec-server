//! Fixed smiley tables.

use speedyspec_wp_text_contracts::{Smilies, TextError};

use crate::small::DEFAULT_SPACES;
use crate::smilies::smilies_search_pattern;

/// A smiley table that never changes, with its search built as `smilies_init()` builds it (default spaces).
pub struct FixedSmilies {
    table: Vec<(Vec<u8>, Vec<u8>)>,
    search: Option<Vec<u8>>,
    includes_base: Vec<u8>,
    site_url: Vec<u8>,
}

impl FixedSmilies {
    /// A table.
    ///
    /// @param table
    ///     `(smiley, image)` pairs.
    /// @param includes_base
    ///     `includes_url()` (the image path is appended).
    /// @param site_url
    ///     `site_url()`.
    /// @return
    ///     The smilies.
    pub fn new(table: Vec<(Vec<u8>, Vec<u8>)>, includes_base: Vec<u8>, site_url: Vec<u8>) -> FixedSmilies {
        let search = smilies_search_pattern(&table, DEFAULT_SPACES);
        FixedSmilies { table, search, includes_base, site_url }
    }

    /// WordPress's default `$wpsmiliestrans` (from `smilies_init()`).
    ///
    /// @param includes_base
    ///     `includes_url()`.
    /// @param site_url
    ///     `site_url()`.
    /// @return
    ///     The smilies.
    pub fn wordpress(includes_base: &[u8], site_url: &[u8]) -> FixedSmilies {
        let table: [(&str, &str); 43] = [
            (":mrgreen:", "mrgreen.png"),
            (":neutral:", "\u{1F610}"),
            (":twisted:", "\u{1F608}"),
            (":arrow:", "\u{27A1}"),
            (":shock:", "\u{1F62F}"),
            (":smile:", "\u{1F642}"),
            (":???:", "\u{1F615}"),
            (":cool:", "\u{1F60E}"),
            (":evil:", "\u{1F47F}"),
            (":grin:", "\u{1F600}"),
            (":idea:", "\u{1F4A1}"),
            (":oops:", "\u{1F633}"),
            (":razz:", "\u{1F61B}"),
            (":roll:", "\u{1F644}"),
            (":wink:", "\u{1F609}"),
            (":cry:", "\u{1F625}"),
            (":eek:", "\u{1F62E}"),
            (":lol:", "\u{1F606}"),
            (":mad:", "\u{1F621}"),
            (":sad:", "\u{1F641}"),
            ("8-)", "\u{1F60E}"),
            ("8-O", "\u{1F62F}"),
            (":-(", "\u{1F641}"),
            (":-)", "\u{1F642}"),
            (":-?", "\u{1F615}"),
            (":-D", "\u{1F600}"),
            (":-P", "\u{1F61B}"),
            (":-o", "\u{1F62E}"),
            (":-x", "\u{1F621}"),
            (":-|", "\u{1F610}"),
            (";-)", "\u{1F609}"),
            ("8O", "\u{1F62F}"),
            (":(", "\u{1F641}"),
            (":)", "\u{1F642}"),
            (":?", "\u{1F615}"),
            (":D", "\u{1F600}"),
            (":P", "\u{1F61B}"),
            (":o", "\u{1F62E}"),
            (":x", "\u{1F621}"),
            (":|", "\u{1F610}"),
            (";)", "\u{1F609}"),
            (":!:", "\u{2757}"),
            (":?:", "\u{2753}"),
        ];
        let pairs = table.iter().map(|(s, i)| (s.as_bytes().to_vec(), i.as_bytes().to_vec())).collect();
        FixedSmilies::new(pairs, includes_base.to_vec(), site_url.to_vec())
    }
}

impl Smilies for FixedSmilies {
    fn search(&self) -> Result<Option<Vec<u8>>, TextError> {
        Ok(self.search.clone())
    }

    fn smileys(&self) -> Result<Vec<Vec<u8>>, TextError> {
        Ok(self.table.iter().map(|(s, _)| s.clone()).collect())
    }

    fn image(&self, smiley: &[u8]) -> Result<Option<Vec<u8>>, TextError> {
        Ok(self.table.iter().find(|(s, _)| s == smiley).map(|(_, i)| i.clone()))
    }

    fn includes_url(&self, path: &[u8]) -> Result<Vec<u8>, TextError> {
        Ok([self.includes_base.as_slice(), path].concat())
    }

    fn site_url(&self) -> Result<Vec<u8>, TextError> {
        Ok(self.site_url.clone())
    }
}

/// No smileys: `convert_smilies()` returns its input.
pub struct NoSmilies;

impl Smilies for NoSmilies {
    fn search(&self) -> Result<Option<Vec<u8>>, TextError> {
        Ok(None)
    }

    fn smileys(&self) -> Result<Vec<Vec<u8>>, TextError> {
        Ok(Vec::new())
    }

    fn image(&self, _smiley: &[u8]) -> Result<Option<Vec<u8>>, TextError> {
        Ok(None)
    }

    fn includes_url(&self, path: &[u8]) -> Result<Vec<u8>, TextError> {
        Ok(path.to_vec())
    }

    fn site_url(&self) -> Result<Vec<u8>, TextError> {
        Ok(Vec::new())
    }
}

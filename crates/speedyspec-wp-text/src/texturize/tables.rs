//! The tables `wptexturize()` builds once per request: translated quotes and dashes, the static replacements and
//! the dynamic patterns.

use crate::preg::{Pattern, PregError};
use crate::small::DEFAULT_SPACES;
use crate::strings::StrReplace;

use super::primes::PrimePatterns;

pub(crate) const OPEN_Q_FLAG: &[u8] = b"<!--oq-->";
pub(crate) const OPEN_SQ_FLAG: &[u8] = b"<!--osq-->";
pub(crate) const APOS_FLAG: &[u8] = b"<!--apos-->";

/// The translated strings and compiled patterns one `wptexturize()` setup produces.
///
/// WordPress builds these in function statics on the first call (and on `$reset`); `TextApi` keeps one of these
/// for as long, so a later locale switch doesn't rebuild them, as in WordPress.
pub struct TexturizeTables {
    pub(crate) opening_quote: Vec<u8>,
    pub(crate) closing_quote: Vec<u8>,
    pub(crate) apos: Vec<u8>,
    pub(crate) prime: Vec<u8>,
    pub(crate) double_prime: Vec<u8>,
    pub(crate) opening_single_quote: Vec<u8>,
    pub(crate) closing_single_quote: Vec<u8>,
    pub(crate) static_replace: StrReplace,
    pub(crate) apos_patterns: Vec<(Pattern, Vec<u8>)>,
    pub(crate) quote_patterns: Vec<(Pattern, Vec<u8>)>,
    pub(crate) dash_patterns: Vec<(Pattern, Vec<u8>)>,
    pub(crate) single_primes: PrimePatterns,
    pub(crate) double_primes: PrimePatterns,
    spaces: Vec<u8>,
    all_patterns: bool,
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn pattern(parts: &[&[u8]]) -> Result<Pattern, PregError> {
    Pattern::try_php(&parts.concat())
}

impl TexturizeTables {
    /// Builds the tables as 7.1.5's `wptexturize()` setup does.
    ///
    /// @param translate
    ///     `_x( $text, $context )`.
    /// @param cockney
    ///     `$wp_cockneyreplace` when a plugin set it (its pairs replace the translated list), else `None`.
    /// @param spaces
    ///     `wp_spaces_regexp()`.
    /// @return
    ///     The tables, or the error a pattern built from `spaces` gives.
    pub fn build(
        translate: &dyn Fn(&str, &str) -> String,
        cockney: Option<Vec<(Vec<u8>, Vec<u8>)>>,
        spaces: &[u8],
    ) -> Result<TexturizeTables, PregError> {
        let t = |text: &str, context: &str| translate(text, context).into_bytes();
        let opening_quote = t("&#8220;", "opening curly double quote");
        let closing_quote = t("&#8221;", "closing curly double quote");
        let apos = t("&#8217;", "apostrophe");
        let prime = t("&#8242;", "prime");
        let double_prime = t("&#8243;", "double prime");
        let opening_single_quote = t("&#8216;", "opening curly single quote");
        let closing_single_quote = t("&#8217;", "closing curly single quote");
        let en_dash = t("&#8211;", "en dash");
        let em_dash = t("&#8212;", "em dash");

        let cockney = cockney.unwrap_or_else(|| {
            let words = t(
                "'tain't,'twere,'twas,'tis,'twill,'til,'bout,'nuff,'round,'cause,'em",
                "Comma-separated list of words to texturize in your language",
            );
            let replacements = t(
                "&#8217;tain&#8217;t,&#8217;twere,&#8217;twas,&#8217;tis,&#8217;twill,&#8217;til,&#8217;bout,&#8217;nuff,&#8217;round,&#8217;cause,&#8217;em",
                "Comma-separated list of replacement words in your language",
            );
            // array_merge of two explode()s, paired by position, as str_replace pairs them; a missing replacement
            // is "".
            let words: Vec<&[u8]> = words.split(|&b| b == b',').collect();
            let replacements: Vec<&[u8]> = replacements.split(|&b| b == b',').collect();
            words
                .iter()
                .enumerate()
                .map(|(i, w)| (w.to_vec(), replacements.get(i).map_or(Vec::new(), |r| r.to_vec())))
                .collect()
        });
        let mut statics = vec![
            (b"...".to_vec(), b"&#8230;".to_vec()),
            (b"``".to_vec(), opening_quote.clone()),
            (b"''".to_vec(), closing_quote.clone()),
            (b" (tm)".to_vec(), b" &#8482;".to_vec()),
        ];
        statics.extend(cockney);
        let static_replace = StrReplace::new(statics);

        let sp = spaces;
        let mut all_patterns = true;
        let mut apos_patterns = Vec::new();
        if apos != b"'" || closing_single_quote != b"'" {
            apos_patterns.push((
                pattern(&[br"/'(\d\d)'(?=\Z|[.,:;!?)}\-\]]|&gt;|", sp, b")/"])?,
                cat(&[APOS_FLAG, b"$1", &closing_single_quote]),
            ));
        } else {
            all_patterns = false;
        }
        if apos != b"'" || closing_quote != b"\"" {
            apos_patterns.push((
                pattern(&[br#"/'(\d\d)"(?=\Z|[.,:;!?)}\-\]]|&gt;|"#, sp, b")/"])?,
                cat(&[APOS_FLAG, b"$1", &closing_quote]),
            ));
        } else {
            all_patterns = false;
        }
        if apos != b"'" {
            apos_patterns.push((Pattern::php(r"/'(?=\d\d(?:\Z|(?![%\d]|[.,]\d)))/"), APOS_FLAG.to_vec()));
        } else {
            all_patterns = false;
        }
        if opening_single_quote != b"'" && closing_single_quote != b"'" {
            apos_patterns.push((
                pattern(&[br"/(?<=\A|", sp, br")'(\d[.,\d]*)'/"])?,
                cat(&[OPEN_SQ_FLAG, b"$1", &closing_single_quote]),
            ));
        } else {
            all_patterns = false;
        }
        if opening_single_quote != b"'" {
            apos_patterns.push((pattern(&[br#"/(?<=\A|[([{"\-]|&lt;|"#, sp, b")'/"])?, OPEN_SQ_FLAG.to_vec()));
        } else {
            all_patterns = false;
        }
        if apos != b"'" {
            apos_patterns.push((
                pattern(&[b"/(?<!", sp, br#")'(?!\Z|[.,:;!?"'(){}[\]\-]|&[lg]t;|"#, sp, b")/"])?,
                APOS_FLAG.to_vec(),
            ));
        } else {
            all_patterns = false;
        }

        let mut quote_patterns = Vec::new();
        if opening_quote != b"\"" && closing_quote != b"\"" {
            quote_patterns.push((
                pattern(&[br"/(?<=\A|", sp, br#")"(\d[.,\d]*)"/"#])?,
                cat(&[OPEN_Q_FLAG, b"$1", &closing_quote]),
            ));
        } else {
            all_patterns = false;
        }
        if opening_quote != b"\"" {
            quote_patterns.push((
                pattern(&[br"/(?<=\A|[([{\-]|&lt;|", sp, br#")"(?!"#, sp, b")/"])?,
                OPEN_Q_FLAG.to_vec(),
            ));
        } else {
            all_patterns = false;
        }

        let dash_patterns = vec![
            (Pattern::php("/---/"), em_dash.clone()),
            (pattern(&[b"/(?<=^|", sp, b")--(?=$|", sp, b")/"])?, em_dash),
            (Pattern::php("/(?<!xn)--/"), en_dash.clone()),
            (pattern(&[b"/(?<=^|", sp, b")-(?=$|", sp, b")/"])?, en_dash),
        ];

        Ok(TexturizeTables {
            single_primes: PrimePatterns::new(b'\'', sp)?,
            double_primes: PrimePatterns::new(b'"', sp)?,
            opening_quote,
            closing_quote,
            apos,
            prime,
            double_prime,
            opening_single_quote,
            closing_single_quote,
            static_replace,
            apos_patterns,
            quote_patterns,
            dash_patterns,
            spaces: spaces.to_vec(),
            all_patterns,
        })
    }

    /// `wp_spaces_regexp()` as the tables were built with it.
    pub fn spaces(&self) -> &[u8] {
        &self.spaces
    }

    /// Whether a hand-written texturizer can run these tables: the spaces fragment is the default and no
    /// translation dropped a dynamic pattern.
    pub fn hand_compatible(&self) -> bool {
        self.all_patterns && self.spaces == DEFAULT_SPACES
    }
}

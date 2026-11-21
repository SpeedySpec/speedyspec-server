/// Which engine compiles a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineChoice {
    /// The `regex` crate: linear time, and combinable into chunks.
    Regex,
    /// `fancy-regex`, for look-around and back-references.
    Fancy,
    /// Neither; the error text from `fancy-regex`.
    None(String),
}

/// Picks the first engine that accepts `pattern`.
pub fn engine_for(pattern: &str) -> EngineChoice {
    if bytes_regex(pattern).is_ok() {
        return EngineChoice::Regex;
    }
    match fancy_regex::Regex::new(pattern) {
        Ok(_) => EngineChoice::Fancy,
        Err(err) => EngineChoice::None(err.to_string()),
    }
}

/// A `regex`-crate pattern with PCRE's non-`/u` semantics: byte subjects, ASCII classes and case folding.
pub(crate) fn bytes_regex(pattern: &str) -> Result<regex::bytes::Regex, regex::Error> {
    regex::bytes::RegexBuilder::new(pattern)
        .unicode(false)
        .build()
}

/// Captures as text; a capture that is not UTF-8 (a decoded `%FF`) is converted lossily.
pub(crate) fn text_captures(caps: &regex::bytes::Captures<'_>) -> Vec<Option<String>> {
    caps.iter()
        .map(|m| m.map(|m| String::from_utf8_lossy(m.as_bytes()).into_owned()))
        .collect()
}

/// One pattern, compiled by the engine that accepts it.
///
/// `fancy-regex` has no byte mode, so a pattern only it compiles runs with Unicode semantics over the subject read as
/// UTF-8 (lossily).
pub(crate) enum Compiled {
    Regex(regex::bytes::Regex),
    Fancy(fancy_regex::Regex),
    None,
}

impl Compiled {
    pub(crate) fn new(pattern: &str) -> Compiled {
        if let Ok(re) = bytes_regex(pattern) {
            return Compiled::Regex(re);
        }
        match fancy_regex::Regex::new(pattern) {
            Ok(re) => Compiled::Fancy(re),
            Err(_) => Compiled::None,
        }
    }

    /// The captures of a match (index 0 is the whole match), or `None`. A `fancy-regex` run that gives up (its
    /// backtracking limit) counts as no match.
    pub(crate) fn captures(&self, subject: &[u8]) -> Option<Vec<Option<String>>> {
        match self {
            Compiled::Regex(re) => re.captures(subject).map(|caps| text_captures(&caps)),
            Compiled::Fancy(re) => {
                let subject = String::from_utf8_lossy(subject);
                let caps = re.captures(subject.as_ref()).ok().flatten()?;
                Some(
                    caps.iter()
                        .map(|m| m.map(|m| m.as_str().to_string()))
                        .collect(),
                )
            }
            Compiled::None => None,
        }
    }

    pub(crate) fn group_count(&self) -> usize {
        match self {
            Compiled::Regex(re) => re.captures_len() - 1,
            Compiled::Fancy(re) => re.captures_len() - 1,
            Compiled::None => 0,
        }
    }
}

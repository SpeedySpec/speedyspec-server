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
    if regex::Regex::new(pattern).is_ok() {
        return EngineChoice::Regex;
    }
    match fancy_regex::Regex::new(pattern) {
        Ok(_) => EngineChoice::Fancy,
        Err(err) => EngineChoice::None(err.to_string()),
    }
}

/// One pattern, compiled by the engine that accepts it.
pub(crate) enum Compiled {
    Regex(regex::Regex),
    Fancy(fancy_regex::Regex),
    None,
}

impl Compiled {
    pub(crate) fn new(pattern: &str) -> Compiled {
        if let Ok(re) = regex::Regex::new(pattern) {
            return Compiled::Regex(re);
        }
        match fancy_regex::Regex::new(pattern) {
            Ok(re) => Compiled::Fancy(re),
            Err(_) => Compiled::None,
        }
    }

    /// The captures of a match (index 0 is the whole match), or `None`. A `fancy-regex` run that gives up (its
    /// backtracking limit) counts as no match.
    pub(crate) fn captures(&self, subject: &str) -> Option<Vec<Option<String>>> {
        match self {
            Compiled::Regex(re) => re.captures(subject).map(|caps| {
                caps.iter()
                    .map(|m| m.map(|m| m.as_str().to_string()))
                    .collect()
            }),
            Compiled::Fancy(re) => re.captures(subject).ok().flatten().map(|caps| {
                caps.iter()
                    .map(|m| m.map(|m| m.as_str().to_string()))
                    .collect()
            }),
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

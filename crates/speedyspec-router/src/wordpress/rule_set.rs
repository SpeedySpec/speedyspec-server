use speedyspec_routing::{
    CompiledDispatch, DispatchStrategy, GuardAnswer, RouteMatch, Rule, RuleGuard, RuleMatch,
};

use super::{QueryTemplate, php_parse_str, php_urldecode_bytes};
use crate::WpSubject;
use crate::pcre::{ends_delimiter_early, with_pcre_dollar};

/// A pattern that matches nothing: what `preg_match` gives a rule whose `#` ends `"#^$match#"` early (it fails).
const NEVER: &str = r"\z.";

/// A WordPress rule set, matched exactly as `WP::parse_request` matches it (Template Method over the steps below).
///
/// 1. An empty path (`''` or `'0'`, as PHP's `empty()` has it) matches only the rule `$`.
/// 2. Each rule is tried as `#^pattern#` is in PHP: `$` also matches before a final newline, and an unescaped `#`
///    makes the rule match nothing. Each rule is tried as `^pattern` against the path, then against its URL-decoded form, before the next rule.
/// 3. Once a rule begins with the requested file (when that file is set and differs from the path), that rule and
///    every later one are tried against `file/path`, as `parse_request`'s `$request_match` stays changed.
/// 4. A rule no Rust engine compiles stops matching: PHP matches from there.
/// 5. A matched rule's guard may reject it (matching resumes at the next rule) or be unable to answer.
pub struct RuleSetMatcher {
    rules: Vec<Rule>,
    templates: Vec<QueryTemplate>,
    dispatch: Box<dyn CompiledDispatch>,
}

impl RuleSetMatcher {
    pub fn compile(
        rules: &[Rule],
        strategy: &dyn DispatchStrategy,
    ) -> Result<RuleSetMatcher, String> {
        let patterns: Vec<String> = rules
            .iter()
            .map(|rule| {
                if ends_delimiter_early(&rule.pattern, b'#') {
                    NEVER.to_string()
                } else {
                    with_pcre_dollar(&format!("^{}", rule.pattern))
                }
            })
            .collect();
        Ok(RuleSetMatcher {
            rules: rules.to_vec(),
            templates: rules
                .iter()
                .map(|rule| QueryTemplate::parse(&rule.query))
                .collect(),
            dispatch: strategy.build(&patterns)?,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    fn matched(&self, index: usize, captures: &[Option<String>]) -> RouteMatch {
        let matched_query = self.templates[index].apply(captures);
        RouteMatch::Rule(RuleMatch {
            matched_rule: self.rules[index].pattern.clone(),
            query_vars: php_parse_str(&matched_query),
            matched_query,
        })
    }

    /// The first rule at index `< end` and `>= from` matching `subject` raw or decoded, raw first on a tie.
    fn first(
        &self,
        subject: &str,
        from: usize,
        end: usize,
    ) -> Option<(usize, Vec<Option<String>>)> {
        let raw = self
            .dispatch
            .find(subject.as_bytes(), from)
            .filter(|(i, _)| *i < end);
        let decoded_subject = php_urldecode_bytes(subject);
        let decoded = if decoded_subject == subject.as_bytes() {
            None
        } else {
            self.dispatch
                .find(&decoded_subject, from)
                .filter(|(i, _)| *i < end)
        };
        match (raw, decoded) {
            (Some(r), Some(d)) => Some(if d.0 < r.0 { d } else { r }),
            (r, d) => r.or(d),
        }
    }

    /// Matches from rule `from` on.
    pub fn find(
        &self,
        subject: &WpSubject,
        from: usize,
        guard: &dyn RuleGuard,
    ) -> Option<RouteMatch> {
        // PHP's `empty()`: `"0"` is empty too.
        if matches!(subject.requested_path.as_str(), "" | "0") {
            if from > 0 {
                return None;
            }
            let index = self.rules.iter().position(|rule| rule.pattern == "$")?;
            return Some(self.matched(index, &[Some(String::new())]));
        }
        let file = &subject.requested_file;
        let anchor = if !file.is_empty() && *file != subject.requested_path {
            self.rules
                .iter()
                .position(|rule| rule.pattern.starts_with(file.as_str()))
        } else {
            None
        }
        .unwrap_or(self.rules.len());
        let anchored = format!("{file}/{}", subject.requested_path);

        let mut from = from;
        loop {
            let found = if from < anchor {
                self.first(&subject.requested_path, from, anchor)
                    .or_else(|| self.first(&anchored, anchor, self.rules.len()))
            } else {
                self.first(&anchored, from, self.rules.len())
            };
            let needs_php = self.dispatch.first_needs_php(from);
            match (found, needs_php) {
                (Some((index, _)), Some(php)) if php < index => {
                    return Some(RouteMatch::NeedsPhp { rule_index: php });
                }
                (None, Some(php)) => return Some(RouteMatch::NeedsPhp { rule_index: php }),
                (None, None) => return None,
                (Some((index, captures)), _) => match guard.check(&self.rules[index], &captures) {
                    GuardAnswer::Accept => return Some(self.matched(index, &captures)),
                    GuardAnswer::Reject => from = index + 1,
                    GuardAnswer::Unavailable => {
                        return Some(RouteMatch::PendingGuard { rule_index: index });
                    }
                },
            }
        }
    }
}
